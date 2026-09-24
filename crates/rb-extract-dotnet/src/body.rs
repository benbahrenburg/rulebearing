//! Method-body dependencies, as `ArchUnitNET`'s `TypeProcessor` phase 7 finds them.
//!
//! - Plan: [Wave 2, Step 3](../../../docs/plans/pending/0002-wave-2-dotnet-python-element-rules.md#23-step-3-attribution-edge-projection-the-net-code-layer-and-defaults-2a)
//!   and § 1.4.2 (the `body`, `call`, `typeof` and `dynamic` rows)
//! - Specification: `ArchUnitNET` 0.13.4 `TypeProcessor.CreateMethodBodyDependencies`,
//!   `MonoCecilMemberExtensions.ScanMethodBody`, `HandleAsync`, `HandleIterator`
//! - Decision: [ADR-0011](../../../docs/adr/0011-read-dotnet-assemblies-not-source.md)
//!
//! | Instruction | Dependency |
//! | --- | --- |
//! | a local variable's type | body type |
//! | `castclass` | cast (`body`) |
//! | `isinst` | type check (`body`) |
//! | `ldtoken` of a type | metadata (`typeof`) |
//! | `box`, `newarr`, `initobj`, `unbox`, `unbox.any`, `ldelem`, `ldobj`, `stelem`, `ldelema`, `stobj` | body type |
//! | a field operand | the field's declaring type (`body`, `member` = the field) |
//! | a method operand (`call`, `callvirt`, `newobj`, `ldftn`, ...) | the method's declaring type (`body`, and a `calls[]` entry); a compiler-generated method (a lambda) is followed into instead |
//!
//! An `async` or iterator method is read through its state machine's `MoveNext`, with the state
//! machine's fields as body types, exactly as `ArchUnitNET` does; a method calling itself is not
//! its own dependency. A literal `Type.GetType("…")`, `Activator.CreateInstance(…, "…")` or
//! `Assembly.Load("…")` adds a `dynamic` dependency on what the string names.

use std::collections::BTreeSet;

use rb_model::DependencyKind;

use crate::codelayer::{Builder, Found, Ref, Target};
use crate::il::Use;
use crate::loader::{MemberRefKind, Method, MethodTarget, Parent, Type};
use crate::metadata::tables::id;
use crate::names::{Generics, Resolved, is_compiler_generated_name};
use crate::sig::Token;

/// `ArchUnitNET`'s `BodyTypeOpCodes`.
const BODY_TYPE_OPCODES: &[u16] = &[0x8C, 0x8D, 0xFE15, 0x79, 0xA5, 0xA3, 0x71, 0xA4, 0x8F, 0x81];
const CASTCLASS: u16 = 0x74;
const ISINST: u16 = 0x75;
const LDTOKEN: u16 = 0xD0;

/// A method one instruction names.
struct Callee {
    /// The declaring type.
    owner: Option<Ref>,
    /// The owner's full name, for the compiler-generated test.
    owner_name: String,
    /// The method name.
    name: String,
    /// The method's Cecil full name, for `calls[]`.
    full_name: String,
    /// The generic method's type arguments (body types).
    args: Vec<Ref>,
    /// The local definition, when the method is defined in the scanned assembly.
    local: Option<u32>,
}

/// A found dependency with its body form set.
fn with_form(mut found: Found, form: &'static str) -> Found {
    found.form = Some(form);
    found
}

impl Builder<'_> {
    /// The body dependencies of `method` of type `owner` in assembly `asm`, and its calls.
    pub(crate) fn body(&self, asm: usize, owner: &Type, method: &Method) -> Vec<Found> {
        let mut found = Vec::new();
        let mut visited: BTreeSet<u32> =
            BTreeSet::from([(u32::from(id::METHOD_DEF) << 24) | method.row]);
        let (scanned, extra) = self.state_machine(asm, method, &mut visited);
        let scanned = scanned.unwrap_or((owner, method));
        for field_ref in extra {
            found.push(with_form(
                self.found(asm, scanned.1, field_ref, DependencyKind::Body, None, None),
                "body-type",
            ));
        }
        self.scan(asm, scanned.0, scanned.1, &mut visited, &mut found, 0);
        found
    }

    /// For an `async` or iterator method, the state machine's `MoveNext` and its field types.
    pub(crate) fn state_machine<'s>(
        &'s self,
        asm: usize,
        method: &Method,
        visited: &mut BTreeSet<u32>,
    ) -> (Option<(&'s Type, &'s Method)>, Vec<Ref>) {
        let loaded = self.universe.assemblies[asm];
        let marker = method.attributes.iter().find_map(|a| {
            match self.attribute_name(asm, a.type_).as_str() {
                "System.Runtime.CompilerServices.AsyncStateMachineAttribute" => {
                    Some(&["__state", "__builder", "__this"][..])
                }
                "System.Runtime.CompilerServices.IteratorStateMachineAttribute" => {
                    Some(&["__state", "__current", "__initialThreadId", "__this"][..])
                }
                _ => None,
            }
        });
        let (Some(excluded), Some(body)) = (marker, &method.body) else {
            return (None, Vec::new());
        };
        for instruction in body.instructions.iter().filter(|i| i.opcode == 0x73) {
            let Some(callee) = self.callee(asm, instruction.token) else {
                continue;
            };
            let Some(Resolved::Def { assembly, row }) =
                callee.owner.as_ref().and_then(Ref::resolved)
            else {
                continue;
            };
            if *assembly != asm {
                continue;
            }
            let Some(machine) = loaded.type_at(*row) else {
                continue;
            };
            if let Some(move_next) = machine.methods.iter().find(|m| m.name == "MoveNext") {
                visited.insert((u32::from(id::METHOD_DEF) << 24) | move_next.row);
                let generics = Self::generics_of(machine, None);
                let fields = machine
                    .fields
                    .iter()
                    .filter(|f| !excluded.iter().any(|suffix| f.name.ends_with(suffix)))
                    .filter_map(|f| self.sig_ref(asm, &f.ty, generics))
                    .collect();
                return (Some((machine, move_next)), fields);
            }
        }
        (None, Vec::new())
    }

    /// Scans one body into `found`, following compiler-generated callees.
    #[expect(
        clippy::too_many_lines,
        reason = "one pass over the instructions in ArchUnitNET's ScanMethodBody order, then the callees"
    )]
    fn scan(
        &self,
        asm: usize,
        owner: &Type,
        method: &Method,
        visited: &mut BTreeSet<u32>,
        found: &mut Vec<Found>,
        depth: u32,
    ) {
        let Some(body) = &method.body else {
            return;
        };
        let generics = Self::generics_of(owner, Some(method));
        for local in &method.locals {
            if let Some(r) = self.sig_ref(asm, local, generics) {
                found.push(with_form(
                    self.found(asm, method, r, DependencyKind::Body, None, None),
                    "body-type",
                ));
            }
        }
        let mut fields_seen = BTreeSet::new();
        let mut callees = Vec::new();
        let mut last_string: Option<&str> = None;
        for instruction in &body.instructions {
            let offset = Some(instruction.offset);
            let table = (instruction.token >> 24) as u8;
            let token_ref = || {
                let token = Token {
                    table,
                    row: instruction.token & 0x00FF_FFFF,
                };
                self.token_ref(asm, token, generics)
            };
            match (instruction.use_, instruction.opcode) {
                (Use::String, _) => {
                    last_string = method
                        .strings
                        .iter()
                        .find(|(o, _)| *o == instruction.offset)
                        .map(|(_, s)| s.as_str());
                    continue;
                }
                (Use::Type | Use::Token, op)
                    if matches!(table, id::TYPE_DEF | id::TYPE_REF | id::TYPE_SPEC) =>
                {
                    let (kind, form) = match op {
                        LDTOKEN => (DependencyKind::Typeof, None),
                        CASTCLASS => (DependencyKind::Body, Some("cast")),
                        ISINST => (DependencyKind::Body, Some("type-check")),
                        op if BODY_TYPE_OPCODES.contains(&op) => {
                            (DependencyKind::Body, Some("body-type"))
                        }
                        _ => continue,
                    };
                    if let Some(r) = token_ref() {
                        let mut f = self.found(asm, method, r, kind, None, offset);
                        f.form = form;
                        found.push(f);
                    }
                }
                (Use::Field | Use::Token, _)
                    if matches!(table, id::FIELD | id::MEMBER_REF)
                        && self.is_field(asm, instruction.token) =>
                {
                    if !fields_seen.insert(instruction.token) {
                        continue;
                    }
                    if let Some((owner_ref, name)) =
                        self.field_owner(asm, instruction.token, generics)
                    {
                        found.push(self.found(
                            asm,
                            method,
                            owner_ref,
                            DependencyKind::Body,
                            Some(name),
                            offset,
                        ));
                    }
                }
                (Use::Call | Use::New | Use::Token, _)
                    if !callees.iter().any(|(t, _, _)| *t == instruction.token) =>
                {
                    callees.push((instruction.token, offset, last_string));
                }
                _ => {}
            }
            last_string = None;
        }
        for (token, offset, literal) in callees {
            if !visited.insert(token) {
                continue;
            }
            let Some(callee) = self.callee(asm, token) else {
                continue;
            };
            for arg in &callee.args {
                found.push(self.found(
                    asm,
                    method,
                    arg.clone(),
                    DependencyKind::Body,
                    None,
                    offset,
                ));
            }
            let generated = is_compiler_generated_name(&callee.name)
                || callee
                    .owner_name
                    .rsplit(['.', '+'])
                    .next()
                    .is_some_and(is_compiler_generated_name);
            if generated {
                if depth < 32
                    && let Some(row) = callee.local
                    && let Some((inner_owner, inner)) = self.universe.assemblies[asm].method_at(row)
                {
                    let (machine, extra) = self.state_machine(asm, inner, visited);
                    let (o, m) = machine.unwrap_or((inner_owner, inner));
                    for r in extra {
                        found.push(with_form(
                            self.found(asm, m, r, DependencyKind::Body, None, None),
                            "body-type",
                        ));
                    }
                    self.scan(asm, o, m, visited, found, depth + 1);
                }
                continue;
            }
            if let Some(dynamic) = literal.and_then(|text| self.dynamic_target(&callee, text)) {
                let mut f = self.found(asm, method, dynamic, DependencyKind::Body, None, offset);
                f.dynamic = true;
                f.form = Some("call");
                found.push(f);
            }
            if let Some(owner_ref) = callee.owner.clone() {
                let mut f = self.found(
                    asm,
                    method,
                    owner_ref,
                    DependencyKind::Body,
                    Some(callee.name.clone()),
                    offset,
                );
                f.call = Some(callee.full_name.clone());
                found.push(f);
            }
        }
    }

    /// What a literal reflection call names: `Type.GetType("Ns.T, Asm")`,
    /// `Activator.CreateInstance("Asm", "Ns.T")`, `Assembly.Load("Asm")`.
    fn dynamic_target(&self, callee: &Callee, literal: &str) -> Option<Ref> {
        match (callee.owner_name.as_str(), callee.name.as_str()) {
            ("System.Type", "GetType")
            | ("System.Activator", "CreateInstance" | "CreateInstanceFrom") => {
                Some(Ref::of(self.named(literal)))
            }
            ("System.Reflection.Assembly", "Load" | "LoadFrom") => {
                let assembly = literal
                    .split(',')
                    .next()
                    .unwrap_or(literal)
                    .trim()
                    .to_owned();
                Some(Ref::of(Target::Type(Resolved::External {
                    full_name: String::new(),
                    assembly: Some(assembly),
                })))
            }
            _ => None,
        }
    }

    fn is_field(&self, asm: usize, token: u32) -> bool {
        let row = token & 0x00FF_FFFF;
        match (token >> 24) as u8 {
            id::FIELD => true,
            id::MEMBER_REF => self.universe.assemblies[asm]
                .member_refs
                .get((row as usize).wrapping_sub(1))
                .is_some_and(|m| matches!(m.kind, MemberRefKind::Field(_))),
            _ => false,
        }
    }

    /// The declaring type and name of a field token.
    fn field_owner(&self, asm: usize, token: u32, generics: Generics<'_>) -> Option<(Ref, String)> {
        let loaded = self.universe.assemblies[asm];
        let row = token & 0x00FF_FFFF;
        match (token >> 24) as u8 {
            id::FIELD => {
                let (owner, field) = loaded.field_at(row)?;
                Some((
                    Ref::of(Target::Type(Resolved::Def {
                        assembly: asm,
                        row: owner.row,
                    })),
                    field.name.clone(),
                ))
            }
            id::MEMBER_REF => {
                let member = loaded.member_refs.get((row as usize).checked_sub(1)?)?;
                let Parent::Type(parent) = member.parent else {
                    return None;
                };
                Some((self.token_ref(asm, parent, generics)?, member.name.clone()))
            }
            _ => None,
        }
    }

    /// The method a call token names.
    fn callee(&self, asm: usize, token: u32) -> Option<Callee> {
        let loaded = self.universe.assemblies[asm];
        let row = token & 0x00FF_FFFF;
        match (token >> 24) as u8 {
            id::METHOD_DEF => self.callee_of(asm, MethodTarget::Def(row), Vec::new()),
            id::MEMBER_REF => self.callee_of(asm, MethodTarget::Ref(row), Vec::new()),
            id::METHOD_SPEC => {
                let spec = loaded.method_specs.get((row as usize).checked_sub(1)?)?;
                let args = spec
                    .args
                    .iter()
                    .filter_map(|a| self.sig_ref(asm, a, Generics::default()))
                    .collect();
                self.callee_of(asm, spec.method, args)
            }
            _ => None,
        }
    }

    fn callee_of(&self, asm: usize, target: MethodTarget, args: Vec<Ref>) -> Option<Callee> {
        let loaded = self.universe.assemblies[asm];
        match target {
            MethodTarget::Def(row) => {
                let (owner, method) = loaded.method_at(row)?;
                Some(Callee {
                    owner: Some(Ref::of(Target::Type(Resolved::Def {
                        assembly: asm,
                        row: owner.row,
                    }))),
                    owner_name: owner.full_name.clone(),
                    name: method.name.clone(),
                    full_name: self.method_full_name(asm, owner, method),
                    args,
                    local: Some(row),
                })
            }
            MethodTarget::Ref(row) => {
                let member = loaded.member_refs.get((row as usize).checked_sub(1)?)?;
                let MemberRefKind::Method(sig) = &member.kind else {
                    return None;
                };
                let Parent::Type(parent) = member.parent else {
                    return None;
                };
                let owner = self.token_ref(asm, parent, Generics::default());
                let owner_name = owner
                    .as_ref()
                    .map(|o| self.target_name(&o.target))
                    .unwrap_or_default();
                let parent_cecil = self
                    .universe
                    .token_name(asm, parent, Generics::default(), true);
                let params: Vec<String> = sig
                    .params
                    .iter()
                    .map(|p| self.universe.sig_name(asm, p, Generics::default(), true))
                    .collect();
                let full_name = format!(
                    "{} {parent_cecil}::{}({})",
                    self.universe
                        .sig_name(asm, &sig.ret, Generics::default(), true),
                    member.name,
                    params.join(",")
                );
                // A reference to a method of a type defined here (a generic display class, say)
                // resolves to the local definition, so a lambda in one can be followed.
                let local = owner
                    .as_ref()
                    .and_then(Ref::resolved)
                    .and_then(|r| match r {
                        Resolved::Def { assembly, row } if *assembly == asm => loaded
                            .type_at(*row)?
                            .methods
                            .iter()
                            .find(|m| {
                                m.name == member.name && m.sig.params.len() == sig.params.len()
                            })
                            .map(|m| m.row),
                        _ => None,
                    });
                Some(Callee {
                    owner,
                    owner_name,
                    name: member.name.clone(),
                    full_name,
                    args,
                    local,
                })
            }
        }
    }
}
