# Copyright (c) 2026 Ben Bahrenburg. MIT licence: see LICENSE.
"""Port ArchUnitNET's element-rule snapshot tests to Rulebearing element rules (conformance gate 2).

- Plan: Wave 2, Step 7 (2C),
  ../../../docs/plans/pending/0002-wave-2-dotnet-python-element-rules.md#27-step-7-gate-2-porting-to-completion-2c
- Decision: [ADR-0009](../../../docs/adr/0009-conformance-suites-as-specification.md) (the upstream
  suite at the pinned tag is the specification)
- Coverage: [ArchUnitNET 0.13.4 tab](../../../docs/artifacts/archunitnet-0.13.4-coverage.md)
- Schema: [`crates/rb-config/src/elements.rs`](../../../crates/rb-config/src/elements.rs), whose
  `VOCABULARY` this tool reads, so a key it writes is a key the parser accepts

Each upstream element test builds rules with the fluent API and ends every rule with
`Assert{No,Any,Only}Violations(helper)` or `AssertException<T>(helper)`; each call appends one
`Query:` block to `Snapshots/<TestClass>.<Test>.verified.txt`, in order. The tool parses the C#
test methods, pairs call *i* with block *i*, translates the fluent chain into an element rule and
reads the expected verdict per object from the block. Helper fields (`helper.RegularClass`) are
resolved by parsing `ArchUnitNETTests/AssemblyTestHelper/*.cs` against the committed fixture graphs
in `conformance/archunitnet/graphs/`; an assembly's full name (`Version=...`, `PublicKeyToken=...`)
is the one the snapshots print. Anything it cannot translate with certainty is written to
`unported.json` with a reason, never guessed.

Tests without a snapshot (the PlantUML tests, for one) are ported by hand into `ported/*.yaml`
files whose first line reads `# Ported from ArchUnitNET <pin> <source> by hand: <why>`. The tool
keeps those files, leaves the tests their cases name (by `id` or `alsoCovers`) out of
`unported.json`, and counts their cases. A non-snapshot test that stays unported keeps a reason
written by hand in `unported.json` (anything but `not-yet: ...`) when the tool is rerun.

Usage: `python3 conformance/archunitnet/tools/port.py [--source <ArchUnitNET checkout>]`. Without
`--source` it clones ArchUnitNET at the tag in `conformance/archunitnet/PIN` into a temporary
directory. Output is deterministic: rerunning it gives byte-identical files.
"""

from __future__ import annotations

import argparse
import json
import re
import shutil
import subprocess
import sys
import tempfile
from dataclasses import dataclass, field, replace
from pathlib import Path
from typing import TYPE_CHECKING

import yaml

if TYPE_CHECKING:
    from collections.abc import Iterator

HERE = Path(__file__).resolve().parent
GATE = HERE.parent
ROOT = GATE.parent.parent
ELEMENTS_RS = ROOT / "crates" / "rb-config" / "src" / "elements.rs"
UPSTREAM = "https://github.com/TNG/ArchUnitNET.git"

ELEMENTS_DIR = "ArchUnitNETTests/Fluent/Syntax/Elements"
HELPER_DIR = "ArchUnitNETTests/AssemblyTestHelper"
STATIC_ARCHITECTURES = "ArchUnitNETTests/StaticTestArchitectures.cs"
SCOPE_GLOBS = (
    "ArchUnitNETTests/Fluent/**/*.cs",
    "ArchUnitNETTests/Domain/PlantUml/*.cs",
    "ArchUnitNETTests/Dependencies/PlantUmlDependenciesTest.cs",
)

ASSERTS = ("AssertNoViolations", "AssertAnyViolations", "AssertOnlyViolations", "AssertException")
PROVIDERS = {
    "Types": ("type", "Types"),
    "Classes": ("class", "Classes"),
    "Interfaces": ("interface", "Interfaces"),
    "Attributes": ("attribute", "Attributes"),
    "Members": ("member", "Members"),
    "FieldMembers": ("field", "Field members"),
    "MethodMembers": ("method", "Method members"),
    "PropertyMembers": ("property", "Property members"),
}
# The nested selector a `...That()` method opens, by suffix, longest first.
THAT_KINDS = (
    ("MethodMembersThat", "method"),
    ("InterfacesThat", "interface"),
    ("AttributesThat", "attribute"),
    ("TypesThat", "type"),
)
CUSTOM = ("FollowCustomPredicate", "FollowCustomCondition")
KEYWORD_TYPES = {
    "string": "System.String",
    "int": "System.Int32",
    "object": "System.Object",
    "bool": "System.Boolean",
}
PAIR = 2  # a named argument is a (name, value) tuple
VACUOUS = "The rule requires positive evaluation, not just absence of violations."
NO_OBJECTS = "There are no objects matching the criteria"
HEADER = (
    "# Ported from ArchUnitNET {pin} {source} by conformance/archunitnet/tools/port.py."
    " Do not edit; rerun the tool.\n"
)
PORTED_COMMENT = (
    "Conformance gate 2 ratchet (docs/adr/0009-conformance-suites-as-specification.md): `ported`"
    " may only rise. Written by conformance/archunitnet/tools/port.py"
    " (docs/plans/pending/0002-wave-2-dotnet-python-element-rules.md, Step 7). `total` counts the"
    " distinct snapshot cases of the element tests (after the tool folds C# overloads with the"
    " same rule and expectation into one case), each unported snapshot block, each in-scope"
    " test with no snapshot as 1, and the cases of the hand-ported files in place of the tests"
    " they cover; `ported` counts the cases in ported/*.yaml; `customPredicate` counts the"
    " unported.json entries whose reason is custom-predicate."
)
UNPORTED_COMMENT = (
    "Upstream ArchUnitNET tests with no case in ported/, each with a reason, written by"
    " conformance/archunitnet/tools/port.py (docs/plans/pending/0002-wave-2-dotnet-python-"
    "element-rules.md, Step 7). `block` is the 1-based snapshot block of `test`, or null for a"
    " whole test that has no snapshot. Reasons: custom-predicate (a C# predicate or condition,"
    " no data-driven case), not-yet: <method or construct> (the element-rule schema or the tool"
    " cannot express it yet), not-yet: non-snapshot (asserted in C#, ported separately); a"
    " non-snapshot test's reason written by hand (wave-3: ..., api-only: ...) is kept."
)
HAND = " by hand: "


class UnportableError(Exception):
    """A construct the tool will not translate; the message is the unported reason."""

    def __init__(self, message: str, *, named: bool = False) -> None:
        """`named` when the message already names the innermost fluent method."""
        super().__init__(message)
        self.named = named


# ---------------------------------------------------------------------------------------------
# Tokens


@dataclass(frozen=True)
class Token:
    """One C# token: kind is `id`, `num`, `str`, `istr` (interpolated), `chr` or `op`."""

    kind: str
    text: str


TOKEN_RE = re.compile(
    r"""
    (?P<ws>\s+)
  | (?P<line>//[^\n]*)
  | (?P<block>/\*.*?\*/)
  | (?P<istr>\$@?"(?:[^"\\{]|\\.|\{[^}]*\})*")
  | (?P<vstr>@"(?:[^"]|"")*")
  | (?P<str>"(?:[^"\\\n]|\\.)*")
  | (?P<chr>'(?:[^'\\]|\\.)+')
  | (?P<num>\d[\d_]*(?:\.\d+)?[A-Za-z]*)
  | (?P<id>@?[A-Za-z_][A-Za-z0-9_]*)
  | (?P<op>=>|==|!=|<=|>=|&&|\|\||\?\?|\?\.|\.\.|[{}()\[\];,.<>=+\-*/%!?:&|^~])
    """,
    re.VERBOSE | re.DOTALL,
)


def tokenize(text: str) -> list[Token]:
    """Splits C# source into tokens, dropping whitespace and comments."""
    tokens: list[Token] = []
    pos = 0
    while pos < len(text):
        match = TOKEN_RE.match(text, pos)
        if match is None:
            message = f"cannot tokenize at {text[pos : pos + 40]!r}"
            raise UnportableError(message)
        pos = match.end()
        kind = match.lastgroup or ""
        if kind in ("ws", "line", "block"):
            continue
        tokens.append(Token("str" if kind == "vstr" else kind, match.group()))
    return tokens


# ---------------------------------------------------------------------------------------------
# Expressions


@dataclass(frozen=True)
class TypeName:
    """A C# type as written: dotted parts, generic arguments or an open arity, tuple items."""

    parts: tuple[str, ...]
    args: tuple[TypeName, ...] = ()
    arity: int = 0
    items: tuple[TypeName, ...] = ()
    array: bool = False

    def text(self) -> str:
        """The type as C# text."""
        if self.items:
            base = "(" + ", ".join(i.text() for i in self.items) + ")"
        else:
            base = ".".join(self.parts)
            if self.args:
                base += "<" + ", ".join(a.text() for a in self.args) + ">"
            elif self.arity:
                base += "<" + "," * (self.arity - 1) + ">"
        return base + ("[]" if self.array else "")


@dataclass(frozen=True)
class Name:
    """An identifier."""

    ident: str


@dataclass(frozen=True)
class Member:
    """`target.name`."""

    target: Expr
    name: str


@dataclass(frozen=True)
class Call:
    """`target.name<types>(args)`, or `name(args)` with no target."""

    target: Expr | None
    name: str
    type_args: tuple[TypeName, ...]
    args: tuple[Expr, ...]


@dataclass(frozen=True)
class Lit:
    """A string, number, boolean or null literal; `raw` is the source text."""

    value: str | int | bool | None
    raw: str


@dataclass(frozen=True)
class Interp:
    """An interpolated string: literal parts and holes."""

    parts: tuple[str | Expr, ...]
    raw: str


@dataclass(frozen=True)
class New:
    """`new T(args) { init }`; `args` or `init` is None when absent."""

    type: TypeName
    args: tuple[Expr, ...] | None
    init: tuple[Expr, ...] | None


@dataclass(frozen=True)
class TypeOf:
    """`typeof(T)`."""

    type: TypeName


@dataclass(frozen=True)
class Tuple:
    """`(a, b)`."""

    items: tuple[Expr, ...]


@dataclass(frozen=True)
class Coll:
    """A collection expression `[a, b]`."""

    items: tuple[Expr, ...]


@dataclass(frozen=True)
class Opaque:
    """Something the tool does not model (a lambda, an operator); `raw` is its source text."""

    raw: str


Expr = Name | Member | Call | Lit | Interp | New | TypeOf | Tuple | Coll | Opaque


def render(expr: Expr) -> str:
    """Prints an expression back as normalised C#."""
    if isinstance(expr, Lit | Interp | Opaque):
        return expr.raw
    if isinstance(expr, Name):
        return expr.ident
    if isinstance(expr, Member):
        return f"{render(expr.target)}.{expr.name}"
    if isinstance(expr, Call):
        head = f"{render(expr.target)}." if expr.target is not None else ""
        types = "<" + ", ".join(t.text() for t in expr.type_args) + ">" if expr.type_args else ""
        return f"{head}{expr.name}{types}({render_list(expr.args)})"
    if isinstance(expr, New):
        text = f"new {expr.type.text()}"
        if expr.args is not None:
            text += f"({render_list(expr.args)})"
        if expr.init is not None:
            text += f" {{ {render_list(expr.init)} }}"
        return text
    return render_other(expr)


def render_list(items: tuple[Expr, ...]) -> str:
    """Comma-separated expressions."""
    return ", ".join(render(a) for a in items)


def render_other(expr: TypeOf | Tuple | Coll) -> str:
    """`typeof(T)`, a tuple or a collection expression."""
    if isinstance(expr, TypeOf):
        return f"typeof({expr.type.text()})"
    if isinstance(expr, Tuple):
        return f"({render_list(expr.items)})"
    return f"[{render_list(expr.items)}]"


class Parser:
    """A recursive-descent parser for the C# expressions the element tests use."""

    def __init__(self, tokens: list[Token]) -> None:
        """Parses `tokens`."""
        self.tokens = tokens
        self.pos = 0

    def peek(self, offset: int = 0) -> Token | None:
        """The token `offset` ahead, if any."""
        index = self.pos + offset
        return self.tokens[index] if index < len(self.tokens) else None

    def at(self, text: str, offset: int = 0) -> bool:
        """Whether the token `offset` ahead is `text`."""
        token = self.peek(offset)
        return token is not None and token.text == text

    def take(self, text: str | None = None) -> Token:
        """Consumes one token, which must be `text` when given."""
        token = self.peek()
        if token is None or (text is not None and token.text != text):
            message = f"parse: expected {text!r}, found {token.text if token else 'end'!r}"
            raise UnportableError(message)
        self.pos += 1
        return token

    def done(self) -> bool:
        """Whether every token was consumed."""
        return self.pos >= len(self.tokens)

    def expression(self) -> Expr:
        """Parses one expression."""
        token = self.peek()
        if token is not None and token.kind == "id" and self.at("=>", 1):
            return self.opaque_until_separator()
        expr = self.postfix(self.primary())
        if self.peek() is not None and not self.at(",") and not self.at(")"):
            nxt = self.peek()
            if nxt is not None and nxt.text not in ("]", "}", ";"):
                return self.opaque_until_separator(prefix=render(expr))
        return expr

    def opaque_until_separator(self, prefix: str = "") -> Expr:
        """Consumes tokens up to a `,` or closing bracket at depth 0, as an opaque expression."""
        depth = 0
        parts = [prefix] if prefix else []
        while (token := self.peek()) is not None:
            if token.text in ("(", "[", "{"):
                depth += 1
            elif token.text in (")", "]", "}"):
                if depth == 0:
                    break
                depth -= 1
            elif token.text in (",", ";") and depth == 0:
                break
            parts.append(token.text)
            self.pos += 1
        return Opaque(" ".join(parts))

    def arguments(self, close: str) -> tuple[Expr, ...]:
        """Parses comma-separated expressions up to `close`."""
        items: list[Expr] = []
        while not self.at(close):
            items.append(self.expression())
            if not self.at(close):
                self.take(",")
        self.take(close)
        return tuple(items)

    def primary(self) -> Expr:
        """Parses a primary expression."""
        token = self.take()
        if token.kind == "str":
            return Lit(string_value(token.text), token.text)
        if token.kind == "istr":
            return interpolated(token.text)
        if token.kind == "num":
            if not token.text.isdigit():
                message = f"not-yet: numeric literal {token.text}"
                raise UnportableError(message)
            return Lit(int(token.text), token.text)
        if token.text == "(":
            items = self.arguments(")")
            return items[0] if len(items) == 1 else Tuple(items)
        if token.text == "[":
            return Coll(self.arguments("]"))
        if token.kind != "id":
            message = f"parse: unexpected {token.text!r}"
            raise UnportableError(message)
        return self.identifier(token.text)

    def identifier(self, ident: str) -> Expr:
        """Parses what an identifier starts: a keyword form, a call or a name."""
        literals: dict[str, bool | None] = {"true": True, "false": False, "null": None}
        if ident in literals:
            return Lit(literals[ident], ident)
        if ident == "typeof":
            self.take("(")
            type_name = self.type_name()
            self.take(")")
            return TypeOf(type_name)
        if ident == "new":
            return self.new_expression()
        return self.call_or_name(None, ident)

    def new_expression(self) -> Expr:
        """Parses `new T(args) { init }`."""
        type_name = self.type_name()
        args = self.arguments(")") if self.at("(") and self.take("(") else None
        init = self.arguments("}") if self.at("{") and self.take("{") else None
        return New(type_name, args, init)

    def call_or_name(self, target: Expr | None, ident: str) -> Expr:
        """Parses `ident`, `ident(args)` or `ident<T>(args)` after an optional target."""
        type_args: tuple[TypeName, ...] = ()
        if self.at("<"):
            save = self.pos
            try:
                self.take("<")
                type_args = self.type_list(">")
            except UnportableError:
                self.pos = save
                type_args = ()
            if not self.at("("):
                self.pos = save
                type_args = ()
        if self.at("("):
            self.take("(")
            return Call(target, ident, type_args, self.arguments(")"))
        if target is None:
            return Name(ident)
        return Member(target, ident)

    def postfix(self, expr: Expr) -> Expr:
        """Parses member accesses and calls after a primary expression."""
        while self.at("."):
            self.take(".")
            name = self.take()
            if name.kind != "id":
                message = f"parse: member name expected after {render(expr)}"
                raise UnportableError(message)
            expr = self.call_or_name(expr, name.text)
        return expr

    def type_list(self, close: str) -> tuple[TypeName, ...]:
        """Parses type arguments up to `close`."""
        items: list[TypeName] = []
        while not self.at(close):
            items.append(self.type_name())
            if not self.at(close):
                self.take(",")
        self.take(close)
        return tuple(items)

    def type_name(self) -> TypeName:
        """Parses a type: dotted name, generic arguments or open arity, tuple, array."""
        if self.at("("):
            self.take("(")
            items = self.type_list(")")
            return TypeName((), items=items, array=self.array_suffix())
        parts = [self.take().text]
        while self.at(".") and (nxt := self.peek(1)) is not None and nxt.kind == "id":
            self.take(".")
            parts.append(self.take().text)
        args: tuple[TypeName, ...] = ()
        arity = 0
        if self.at("<"):
            self.take("<")
            if self.at(">") or self.at(","):
                arity = 1
                while self.at(","):
                    self.take(",")
                    arity += 1
                self.take(">")
            else:
                args = self.type_list(">")
        return TypeName(tuple(parts), args, arity, array=self.array_suffix())

    def array_suffix(self) -> bool:
        """Consumes `[]` when present."""
        if self.at("[") and self.at("]", 1):
            self.take("[")
            self.take("]")
            return True
        return False


def string_value(raw: str) -> str:
    """The value of a regular or verbatim C# string literal."""
    if raw.startswith("@"):
        return raw[2:-1].replace('""', '"')
    return str(json.loads(raw))


def interpolated(raw: str) -> Interp:
    """Parses `$"...{expr}..."` into literal parts and holes."""
    body = raw[raw.index('"') + 1 : -1]
    parts: list[str | Expr] = []
    for index, piece in enumerate(re.split(r"\{([^}]*)\}", body)):
        if index % 2:
            parser = Parser(tokenize(piece))
            parts.append(parser.expression())
        elif piece:
            parts.append(piece)
    return Interp(tuple(parts), raw)


def parse_expression(tokens: list[Token]) -> Expr:
    """Parses a whole token list as one expression."""
    parser = Parser(tokens)
    expr = parser.expression()
    if not parser.done():
        message = f"parse: trailing tokens after {render(expr)}"
        raise UnportableError(message)
    return expr


def chain(expr: Expr) -> tuple[Expr, list[Call]]:
    """Unwinds `a.B().C()` into its root and its calls, in call order."""
    calls: list[Call] = []
    while isinstance(expr, Call) and expr.target is not None:
        calls.append(expr)
        expr = expr.target
    if isinstance(expr, Call):
        calls.append(expr)
        calls.reverse()
        return Call(None, expr.name, expr.type_args, expr.args), calls[1:]
    calls.reverse()
    return expr, calls


# ---------------------------------------------------------------------------------------------
# Statements and methods


@dataclass
class Statement:
    """One statement's tokens; `control` when it sits inside or is a control-flow statement."""

    tokens: list[Token]
    control: bool


CONTROL = {"foreach", "for", "if", "else", "while", "using", "try", "catch", "finally", "lock"}


def statements(tokens: list[Token]) -> list[Statement]:
    """Splits a method body into statements, flattening nested blocks."""
    out: list[Statement] = []
    current: list[Token] = []
    depth = 0
    blocks: list[bool] = []
    for token in tokens:
        in_control = any(blocks)
        if depth == 0 and token.text == "{" and (not current or current[0].text in CONTROL):
            blocks.append(bool(current))
            if current:
                out.append(Statement(current, control=True))
            current = []
            continue
        if depth == 0 and token.text == "}" and not current:
            if blocks:
                blocks.pop()
            continue
        if token.text in ("(", "[", "{"):
            depth += 1
        elif token.text in (")", "]", "}"):
            depth -= 1
        if depth == 0 and token.text == ";":
            out.append(Statement(current, control=in_control or current[0].text in CONTROL))
            current = []
            continue
        current.append(token)
    return out


@dataclass
class Method:
    """A test method: its name and body tokens."""

    name: str
    body: list[Token]


def matching(tokens: list[Token], start: int) -> int:
    """The index of the bracket that closes the one at `start`."""
    pairs = {"{": "}", "(": ")", "[": "]"}
    opener = tokens[start].text
    depth = 0
    for index in range(start, len(tokens)):
        if tokens[index].text == opener:
            depth += 1
        elif tokens[index].text == pairs[opener]:
            depth -= 1
            if depth == 0:
                return index
    message = "unbalanced brackets"
    raise UnportableError(message)


def test_methods(tokens: list[Token]) -> list[Method]:
    """Every `[Fact]` or `[Theory]` method, in source order."""
    methods: list[Method] = []
    index = 0
    while index < len(tokens):
        if tokens[index].text == "[" and index + 1 < len(tokens):
            ident = tokens[index + 1].text
            if ident in ("Fact", "Theory"):
                index = matching(tokens, index) + 1
                while tokens[index].text == "[":
                    index = matching(tokens, index) + 1
                while tokens[index + 1].text != "(":
                    index += 1
                name = tokens[index].text
                body_start = matching(tokens, index + 1) + 1
                body_end = matching(tokens, body_start)
                methods.append(Method(name, tokens[body_start + 1 : body_end]))
                index = body_end
        index += 1
    return methods


def usings(tokens: list[Token]) -> list[str]:
    """The namespaces a file imports with `using X.Y;` (not `using static`)."""
    found: list[str] = []
    for index, token in enumerate(tokens):
        if token.text != "using" or index + 1 >= len(tokens) or tokens[index + 1].kind != "id":
            continue
        if tokens[index + 1].text == "static" or tokens[index + 1].text == "var":
            continue
        end = index + 1
        while end < len(tokens) and tokens[end].text != ";":
            if tokens[end].text in ("=", "("):
                break
            end += 1
        if end < len(tokens) and tokens[end].text == ";":
            found.append("".join(t.text for t in tokens[index + 1 : end]))
    return found


def first_class(tokens: list[Token]) -> str:
    """The name of the first class a file declares."""
    for index, token in enumerate(tokens[:-1]):
        if token.text == "class":
            return tokens[index + 1].text
    message = "no class in file"
    raise UnportableError(message)


# ---------------------------------------------------------------------------------------------
# The fixture graphs


@dataclass(frozen=True)
class TypeVal:
    """A type: its full name and, when the graphs know it, its record."""

    full_name: str
    name: str
    namespace: str | None
    assembly: str | None


@dataclass(frozen=True)
class MemberVal:
    """A member: its full name, name, declaring type, kind and return type."""

    full_name: str
    name: str
    declaring: str
    kind: str
    return_type: str | None


@dataclass(frozen=True)
class AssemblyVal:
    """An assembly, by simple name."""

    name: str


Value = TypeVal | MemberVal | AssemblyVal | str | int | bool | None


@dataclass
class Graphs:
    """Every committed fixture graph, by assembly."""

    types: dict[str, list[TypeVal]] = field(default_factory=dict)
    kinds: dict[str, str] = field(default_factory=dict)
    members: dict[str, list[MemberVal]] = field(default_factory=dict)
    external: set[str] = field(default_factory=set)
    identities: dict[str, str] = field(default_factory=dict)

    def all_type_names(self) -> set[str]:
        """Every type full name, in any assembly, plus the external types they depend on."""
        names = {t.full_name for types in self.types.values() for t in types}
        return names | self.external

    def find_type(self, full_name: str) -> TypeVal:
        """The type with this full name, or an external stand-in."""
        for assembly in sorted(self.types):
            for record in self.types[assembly]:
                if record.full_name == full_name:
                    return record
        last = re.split(r"[.+]", full_name)[-1]
        return TypeVal(full_name, last, None, None)

    def objects(self, assemblies: list[str]) -> list[str]:
        """Every type and member full name in these assemblies, longest first."""
        names = {t.full_name for a in assemblies for t in self.types.get(a, [])}
        names |= {m.full_name for a in assemblies for m in self.members.get(a, [])}
        return sorted(names, key=lambda n: (-len(n), n))


def load_graphs(directory: Path) -> Graphs:
    """Reads `graphs/*.json`."""
    graphs = Graphs()
    for path in sorted(directory.glob("*.json")):
        code = json.loads(path.read_text(encoding="utf-8"))["code"]
        # Referenced stubs name what the assembly depends on; they are not its types.
        code["types"] = [t for t in code["types"] if not t.get("referenced")]
        for record in code["types"]:
            assembly = str(record["assembly"])
            value = TypeVal(
                str(record["fullName"]),
                str(record["name"]),
                str(record.get("namespace", "")),
                assembly,
            )
            graphs.types.setdefault(assembly, []).append(value)
            graphs.kinds[value.full_name] = str(record["kind"])
            for dependency in record.get("dependencies", []):
                target = str(dependency["target"])
                if "<" not in target and "[" not in target:
                    graphs.external.add(target)
        # One graph per assembly: a member belongs to the assembly of the file it is in.
        file_assemblies = {str(r["assembly"]) for r in code["types"]}
        if len(file_assemblies) != 1:
            message = f"{path.name}: expected the types of one assembly"
            raise SystemExit(message)
        members = graphs.members.setdefault(file_assemblies.pop(), [])
        for record in code["members"]:
            return_type = record.get("returnType")
            members.append(
                MemberVal(
                    str(record["fullName"]),
                    str(record["name"]),
                    str(record["declaringType"]),
                    str(record["kind"]),
                    str(return_type) if return_type else None,
                ),
            )
    return graphs


def harvest_identities(snapshots: Path, graphs: Graphs) -> None:
    """Records each fixture assembly's full name as ArchUnitNET printed it in the snapshots."""
    pattern = re.compile(
        r"\b(\w+), (Version=\d+\.\d+\.\d+\.\d+, Culture=\w+,"
        r" PublicKeyToken=(?:[0-9a-f]{16}|null))\b",
    )
    seen: dict[str, set[str]] = {}
    for path in sorted(snapshots.glob("*.verified.txt")):
        for match in pattern.finditer(path.read_text(encoding="utf-8-sig")):
            if match.group(1) in graphs.types:
                seen.setdefault(match.group(1), set()).add(f"{match.group(1)}, {match.group(2)}")
    graphs.identities = {name: next(iter(v)) for name, v in seen.items() if len(v) == 1}


# ---------------------------------------------------------------------------------------------
# Helpers: the fields tests read as `helper.X`


@dataclass
class Helper:
    """One `*AssemblyTestHelper` class: its architecture, usings and field expressions."""

    name: str
    architecture: str
    usings: list[str]
    fields: dict[str, Expr]
    base: str | None


@dataclass
class World:
    """Everything a translation reads: graphs, architectures and helpers."""

    graphs: Graphs
    architectures: dict[str, list[str]]
    helpers: dict[str, Helper]
    cache: dict[tuple[str, str], Value] = field(default_factory=dict)

    def resolve_type(self, type_name: TypeName, namespaces: list[str]) -> TypeVal:
        """The full name of a C# type as ArchUnitNET prints it, found in the graphs."""
        if type_name.args:
            # A closed generic type: `MatchesType` compares the open type, then each argument,
            # which is the instantiation's full name as the graphs spell it (`G`1<A>`).
            open_name = replace(type_name, args=(), arity=len(type_name.args))
            open_type = self.resolve_type(open_name, namespaces)
            args = ",".join(self.resolve_type(a, namespaces).full_name for a in type_name.args)
            return TypeVal(
                f"{open_type.full_name}<{args}>",
                open_type.name,
                open_type.namespace,
                open_type.assembly,
            )
        if type_name.items or type_name.array:
            message = f"not-yet: type argument {type_name.text()}"
            raise UnportableError(message)
        if len(type_name.parts) == 1 and type_name.parts[0] in KEYWORD_TYPES:
            return self.graphs.find_type(KEYWORD_TYPES[type_name.parts[0]])
        known = self.graphs.all_type_names()
        arity = f"`{type_name.arity}" if type_name.arity else ""
        parts = list(type_name.parts)
        found: set[str] = set()
        for namespace in ["", *namespaces]:
            for split in range(len(parts)):
                prefix = ".".join([p for p in [namespace, *parts[:split]] if p])
                nested = "+".join(parts[split:]) + arity
                candidate = f"{prefix}.{nested}" if prefix else nested
                if candidate in known:
                    found.add(candidate)
        if len(found) != 1:
            message = f"unresolved type {type_name.text()} ({len(found)} candidates)"
            raise UnportableError(message)
        return self.graphs.find_type(found.pop())

    def helper_field(self, helper: Helper, name: str) -> Value:
        """The value of `helper.<name>`, from the helper or its base class."""
        key = (helper.name, name)
        if key in self.cache:
            return self.cache[key]
        current: Helper | None = helper
        while current is not None and name not in current.fields:
            current = self.helpers.get(current.base) if current.base else None
        if current is None:
            message = f"unresolved helper field {helper.name}.{name}"
            raise UnportableError(message)
        value = self.evaluate(current.fields[name], helper, {})
        self.cache[key] = value
        return value

    def assemblies(self, helper: Helper) -> list[str]:
        """The assemblies the helper's architecture loads."""
        current: Helper | None = helper
        while current is not None and not current.architecture:
            current = self.helpers.get(current.base) if current.base else None
        if current is None or current.architecture not in self.architectures:
            message = f"no architecture for helper {helper.name}"
            raise UnportableError(message)
        return self.architectures[current.architecture]

    def evaluate(self, expr: Expr, helper: Helper, local: dict[str, Expr]) -> Value:
        """Evaluates a value expression: a helper field, a literal, `typeof`, a property."""
        if isinstance(expr, Lit):
            return expr.value
        if isinstance(expr, Interp):
            return "".join(
                p if isinstance(p, str) else self.text(p, helper, local) for p in expr.parts
            )
        if isinstance(expr, TypeOf):
            return self.resolve_type(expr.type, helper.usings)
        if isinstance(expr, Name):
            if expr.ident in local:
                return self.evaluate(local[expr.ident], helper, local)
            return self.helper_field(helper, expr.ident)
        if isinstance(expr, Member | Call):
            return self.access(expr, helper, local)
        message = f"not-yet: value {render(expr)}"
        raise UnportableError(message)

    def access(self, expr: Member | Call, helper: Helper, local: dict[str, Expr]) -> Value:
        """Evaluates `x.Property` or `x.Method(...)`."""
        if isinstance(expr, Call):
            return self.call(expr, helper, local)
        return self.member(expr, helper, local)

    def text(self, expr: Expr, helper: Helper, local: dict[str, Expr]) -> str:
        """Evaluates an expression that must give a string."""
        value = self.evaluate(expr, helper, local)
        if not isinstance(value, str):
            message = f"not-yet: non-string value {render(expr)}"
            raise UnportableError(message)
        return value

    def member(self, expr: Member, helper: Helper, local: dict[str, Expr]) -> Value:
        """Evaluates `x.Property`."""
        if isinstance(expr.target, Name) and expr.target.ident == "helper":
            return self.helper_field(helper, expr.name)
        if isinstance(expr.target, Member) and expr.target.name == "Namespace":
            owner = self.evaluate(expr.target.target, helper, local)
            # `Namespace.FullName` is the dotted namespace; `Namespace.Name` is the same text
            # only for a one-segment namespace, the one case the tool can be sure of.
            namespace = owner.namespace if isinstance(owner, TypeVal) else None
            if namespace and (
                expr.name == "FullName" or (expr.name, "." in namespace) == ("Name", False)
            ):
                return namespace
        target = self.evaluate(expr.target, helper, local)
        return self.property(target, expr.name, render(expr))

    def property(self, target: Value, name: str, text: str) -> Value:
        """A property of a resolved value."""
        if isinstance(target, AssemblyVal) and name in ("Name", "FullName"):
            return target.name if name == "Name" else self.identity(target.name)
        if isinstance(target, MemberVal) and name == "ReturnType" and target.return_type:
            return self.graphs.find_type(target.return_type)
        if isinstance(target, TypeVal | MemberVal) and name in ("Name", "FullName"):
            return target.name if name == "Name" else target.full_name
        if isinstance(target, TypeVal) and target.assembly and name == "Assembly":
            return AssemblyVal(target.assembly)
        if isinstance(target, TypeVal) and target.assembly and name == "AssemblyQualifiedName":
            return f"{target.full_name}, {self.identity(target.assembly)}"
        message = f"not-yet: value {text}"
        raise UnportableError(message)

    def identity(self, assembly: str) -> str:
        """The assembly's full name, as the snapshots print it."""
        if assembly not in self.graphs.identities:
            message = f"not-yet: full name of assembly {assembly} unknown"
            raise UnportableError(message)
        return self.graphs.identities[assembly]

    def call(self, expr: Call, helper: Helper, local: dict[str, Expr]) -> Value:
        """Evaluates the architecture lookups the helpers and tests use."""
        text = render(expr)
        getters = ("GetClassOfType", "GetITypeOfType", "GetInterfaceOfType", "GetAttributeOfType")
        if expr.name in getters and len(expr.args) == 1:
            value = self.evaluate(expr.args[0], helper, local)
            if isinstance(value, TypeVal) and value.assembly in self.assemblies(helper):
                return value
        if expr.name == "First" and not expr.args and expr.target is not None:
            candidates = self.candidates(expr.target, helper, local)
            if len(candidates) == 1:
                return candidates[0]
        message = f"not-yet: value {text}"
        raise UnportableError(message)

    def candidates(self, expr: Expr, helper: Helper, local: dict[str, Expr]) -> list[Value]:
        """What a `.First()` would pick from: every match, so a unique one can be required."""
        assemblies = self.assemblies(helper)
        if (
            isinstance(expr, Member)
            and expr.name == "Assemblies"
            and isinstance(expr.target, Member)
            and expr.target.name == "Architecture"
        ):
            return [AssemblyVal(a) for a in assemblies]
        if not isinstance(expr, Call) or len(expr.args) != 1 or expr.target is None:
            return []
        wanted = self.evaluate(expr.args[0], helper, local)
        if expr.name == "WhereNameIs" and isinstance(expr.target, Member):
            return self.where_name_is(expr.target.name, wanted, assemblies)
        return self.members_with_name(expr, wanted, helper, local)

    def where_name_is(self, collection: str, wanted: Value, assemblies: list[str]) -> list[Value]:
        """`Architecture.Classes.WhereNameIs(n)` / `.MethodMembers.WhereNameIs(n)`."""
        if collection == "Classes":
            return [
                t
                for a in assemblies
                for t in self.graphs.types.get(a, [])
                if t.name == wanted and self.graphs.kinds.get(t.full_name) == "class"
            ]
        if collection == "MethodMembers":
            return [
                m
                for a in assemblies
                for m in self.graphs.members.get(a, [])
                if m.name == wanted and m.kind in ("method", "constructor")
            ]
        return []

    def members_with_name(
        self,
        expr: Call,
        wanted: Value,
        helper: Helper,
        local: dict[str, Expr],
    ) -> list[Value]:
        """`T.GetMethodMembersWithName(n)` and its property and field twins."""
        assemblies = self.assemblies(helper)
        kinds = {
            "GetMethodMembersWithName": ("method", "constructor"),
            "GetPropertyMembersWithName": ("property",),
            "GetFieldMembersWithName": ("field",),
        }
        if expr.name in kinds and expr.target is not None:
            owner = self.evaluate(expr.target, helper, local)
            if isinstance(owner, TypeVal):
                return [
                    m
                    for a in assemblies
                    for m in self.graphs.members.get(a, [])
                    if m.declaring == owner.full_name
                    and m.name == wanted
                    and m.kind in kinds[expr.name]
                ]
        return []


def class_body(tokens: list[Token], name: str) -> list[Token]:
    """The tokens inside `class <name> ... { }`."""
    for index, token in enumerate(tokens[:-1]):
        if token.text == "class" and tokens[index + 1].text == name:
            start = index
            while tokens[start].text != "{":
                start += 1
            return tokens[start + 1 : matching(tokens, start)]
    message = f"class {name} not found"
    raise UnportableError(message)


def parse_helper(path: Path) -> Helper:
    """Reads one helper class: its architecture property, field initialisers, constructor."""
    tokens = tokenize(path.read_text(encoding="utf-8-sig"))
    name = first_class(tokens)
    index = [t.text for t in tokens].index(name)
    base = tokens[index + 2].text if tokens[index + 1].text == ":" else None
    body = class_body(tokens, name)
    helper = Helper(name, "", usings(tokens), {}, base)
    for member in class_members(body):
        texts = [t.text for t in member]
        if name in texts and texts[texts.index(name) + 1 :][:1] == ["("] and "{" in texts:
            for statement in statements(member[texts.index("{") + 1 : -1]):
                assign(helper, statement.tokens)
        elif "=>" in texts:
            arrow = texts.index("=>")
            if texts[arrow + 1 : arrow + 3] == ["StaticTestArchitectures", "."]:
                helper.architecture = texts[arrow + 3]
        elif "=" in texts and "(" not in texts[: texts.index("=")]:
            equals = texts.index("=")
            helper.fields[texts[equals - 1]] = parse_expression(member[equals + 1 :])
    return helper


def assign(helper: Helper, tokens: list[Token]) -> None:
    """Records `Name = expr;` from a constructor."""
    texts = [t.text for t in tokens]
    if texts[1:2] == ["="] and tokens[0].kind == "id":
        helper.fields[texts[0]] = parse_expression(tokens[2:])


def class_members(body: list[Token]) -> Iterator[list[Token]]:
    """Splits a class body into member declarations (without the trailing `;`)."""
    current: list[Token] = []
    index = 0
    while index < len(body):
        token = body[index]
        if token.text == "{":
            end = matching(body, index)
            current.extend(body[index : end + 1])
            index = end + 1
            if not (index < len(body) and body[index].text in (";", ")", ",", ".")):
                yield current
                current = []
            continue
        if token.text == ";":
            yield current
            current = []
        elif token.text in ("(", "["):
            end = matching(body, index)
            current.extend(body[index : end + 1])
            index = end + 1
            continue
        else:
            current.append(token)
        index += 1


def parse_architectures(path: Path, graphs: Graphs) -> dict[str, list[str]]:
    """`StaticTestArchitectures.X` to the assemblies `LoadAssemblies(typeof(T).Assembly)` loads."""
    tokens = tokenize(path.read_text(encoding="utf-8-sig"))
    body = class_body(tokens, "StaticTestArchitectures")
    world = World(graphs, {}, {})
    out: dict[str, list[str]] = {}
    for member in class_members(body):
        texts = [t.text for t in member]
        if "=" not in texts or "LoadAssemblies" not in texts:
            continue
        name = texts[texts.index("=") - 1]
        expr = parse_expression(member[texts.index("=") + 1 :])
        _, calls = chain(expr)
        load = next(c for c in calls if c.name == "LoadAssemblies")
        assemblies: list[str] = []
        for arg in load.args:
            if not (isinstance(arg, Member) and isinstance(arg.target, TypeOf)):
                assemblies = []
                break
            try:
                found = world.resolve_type(arg.target.type, usings(tokens))
            except UnportableError:
                assemblies = []
                break
            if found.assembly is None:
                assemblies = []
                break
            assemblies.append(found.assembly)
        if assemblies:
            out[name] = assemblies
    return out


# ---------------------------------------------------------------------------------------------
# The element-rule vocabulary, read from elements.rs


@dataclass(frozen=True)
class Concept:
    """One VOCABULARY row: canonical name, value kind, whether `where` may use it."""

    name: str
    value_kind: str
    in_where: bool


def load_vocabulary(path: Path) -> dict[str, Concept]:
    """Parses `VOCABULARY` from elements.rs."""
    text = path.read_text(encoding="utf-8")
    block = text[text.index("pub const VOCABULARY") :]
    block = block[: block.index("];")]
    rows = re.findall(
        r'\(\s*"([A-Za-z]*)",\s*Concept::\w+,\s*ValueKind::(\w+),\s*(true|false),?\s*\)',
        block,
    )
    if not rows:
        message = f"no VOCABULARY rows in {path}"
        raise SystemExit(message)
    return {name: Concept(name, kind, flag == "true") for name, kind, flag in rows}


def lower_first(text: str) -> str:
    """`BeAbstract` -> `beAbstract`."""
    return text[:1].lower() + text[1:]


def split_key(key: str, side: str) -> tuple[str, bool, bool]:
    """Python twin of elements.rs `split_key`: canonical name, negation, selector."""
    prefixes = (("areNot", True), ("doNot", True), ("are", False))
    if side == "should":
        prefixes = (("notBe", True), ("not", True), ("be", False))
    rest, negated = key, False
    for prefix, negation in prefixes:
        if key.startswith(prefix):
            rest, negated = lower_first(key[len(prefix) :]), negation
            break
    selector = False
    for suffix in ("TypesThat", "That"):
        if rest.endswith(suffix):
            rest, selector = rest[: -len(suffix)], True
            break
    aliases = {
        "constructors": ("constructor", False),
        "noConstructors": ("constructor", True),
        "noConstructor": ("constructor", True),
        "haveNoGetter": ("haveGetter", True),
        "haveNoSetter": ("haveSetter", True),
        "types": ("", False),
        "methodMembers": ("", False),
    }
    base, flip = aliases.get(rest, (rest, False))
    return base, negated ^ flip, selector


# ---------------------------------------------------------------------------------------------
# Translation of one fluent chain


Yaml = dict[str, "Yaml"] | list["Yaml"] | str | int | bool | None


@dataclass
class Context:
    """What a translation reads: the world, the helper, locals and the file's usings."""

    world: World
    vocabulary: dict[str, Concept]
    helper: Helper
    local: dict[str, Expr]
    namespaces: list[str]

    def value(self, expr: Expr) -> Value:
        """Evaluates a value with the test file's usings for `typeof`."""
        if isinstance(expr, TypeOf):
            return self.world.resolve_type(expr.type, self.namespaces)
        if isinstance(expr, Name) and expr.ident in self.local:
            return self.value(self.local[expr.ident])
        return self.world.evaluate(expr, self.helper, self.local)


def expand(ctx: Context, expr: Expr) -> Expr:
    """Replaces a local variable by the expression it was last assigned."""
    while isinstance(expr, Name) and expr.ident in ctx.local:
        expr = ctx.local[expr.ident]
    return expr


def inline(expr: Expr, local: dict[str, Expr]) -> Expr:
    """The expression with every local variable replaced by the value it was assigned."""
    if isinstance(expr, Name):
        return local.get(expr.ident, expr)
    if isinstance(expr, Member):
        return Member(inline(expr.target, local), expr.name)
    if isinstance(expr, Call):
        target = inline(expr.target, local) if expr.target is not None else None
        args = tuple(inline(a, local) for a in expr.args)
        return Call(target, expr.name, expr.type_args, args)
    if isinstance(expr, New):
        args_ = None if expr.args is None else tuple(inline(a, local) for a in expr.args)
        init = None if expr.init is None else tuple(inline(a, local) for a in expr.init)
        return New(expr.type, args_, init)
    if isinstance(expr, Tuple | Coll):
        return type(expr)(tuple(inline(a, local) for a in expr.items))
    return expr


def is_provider(ctx: Context, expr: Expr) -> bool:
    """Whether the expression is an object provider: `Types()...`."""
    root, _ = chain(expand(ctx, expr))
    return isinstance(root, Call) and root.name in PROVIDERS and not root.args


def items(ctx: Context, args: tuple[Expr, ...]) -> list[Expr]:
    """Flattens `params` arguments and `new List<T> { ... }` / `[...]` into their items."""
    out: list[Expr] = []
    for arg in args:
        expr = expand(ctx, arg)
        if isinstance(expr, New) and expr.type.parts[-1:] in (("List",), ("HashSet",)):
            if expr.args:
                message = f"not-yet: collection constructor {render(expr)}"
                raise UnportableError(message)
            out.extend(expr.init or ())
        elif isinstance(expr, Coll):
            out.extend(expr.items)
        else:
            out.append(expr)
    return out


def object_name(ctx: Context, expr: Expr) -> str:
    """The full name of a type or member argument."""
    value = ctx.value(expr)
    if isinstance(value, TypeVal | MemberVal):
        return value.full_name
    message = f"not-yet: object argument {render(expr)}"
    raise UnportableError(message)


def objects(ctx: Context, args: tuple[Expr, ...]) -> Yaml:
    """An Objects operand: a nested selector, or a list of full names."""
    if len(args) == 1 and is_provider(ctx, args[0]):
        return selector(ctx, expand(ctx, args[0]))
    if any(is_provider(ctx, a) for a in args):
        message = "not-yet: mixed object and selector arguments"
        raise UnportableError(message)
    return [object_name(ctx, e) for e in items(ctx, args)]


def names(ctx: Context, args: tuple[Expr, ...]) -> Yaml:
    """A Names operand: one string, or a list when there are several."""
    out: list[Yaml] = []
    for expr in items(ctx, args):
        value = ctx.value(expr)
        if isinstance(value, AssemblyVal):
            value = ctx.world.identity(value.name)
        if not isinstance(value, str):
            message = f"not-yet: name argument {render(expr)}"
            raise UnportableError(message)
        out.append(value)
    if len(out) == 1:
        return out[0]
    return out


def literal(ctx: Context, expr: Expr) -> Yaml:
    """An attribute argument as literal text: string, number, boolean, type full name, null."""
    value = ctx.value(expr)
    if isinstance(value, TypeVal):
        return value.full_name
    if value is None or isinstance(value, str | int | bool):
        return value
    message = f"not-yet: attribute argument {render(expr)}"
    raise UnportableError(message)


def named(ctx: Context, args: tuple[Expr, ...]) -> Yaml:
    """Named attribute arguments `("Name", value)` as a mapping."""
    mapping: dict[str, Yaml] = {}
    for expr in items(ctx, args):
        if not isinstance(expr, Tuple) or len(expr.items) != PAIR:
            message = f"not-yet: named argument {render(expr)}"
            raise UnportableError(message)
        key = ctx.value(expr.items[0])
        if not isinstance(key, str) or key in mapping:
            message = f"not-yet: named argument {render(expr)}"
            raise UnportableError(message)
        mapping[key] = literal(ctx, expr.items[1])
    return mapping


def argument_values(ctx: Context, args: tuple[Expr, ...]) -> Yaml:
    """Positional attribute argument values."""
    values: list[Yaml] = [literal(ctx, e) for e in items(ctx, args)]
    return values


def attribute(ctx: Context, expr: Expr) -> Yaml:
    """The attribute an argument test names: a full name or a selector."""
    if is_provider(ctx, expr):
        return selector(ctx, expand(ctx, expr))
    return object_name(ctx, expr)


def operand(ctx: Context, concept: Concept, call: Call) -> Yaml:
    """The YAML value of one predicate or condition call, by value kind."""
    kind, args = concept.value_kind, call.args
    simple = {
        "Names": names,
        "Objects": objects,
        "ArgumentValues": argument_values,
        "NamedArgumentValues": named,
    }
    if kind in simple:
        return simple[kind](ctx, args)
    if kind == "Flag" and not args:
        return True
    if kind == "Pattern" and isinstance(value := names(ctx, args), str):
        return value
    if kind in ("AttributeArguments", "AttributeNamedArguments") and args:
        values = (
            argument_values(ctx, args[1:]) if kind == "AttributeArguments" else named(ctx, args[1:])
        )
        return {"attribute": attribute(ctx, args[0]), "arguments": values}
    message = f"not-yet: {call.name}"
    raise UnportableError(message)


def test_entry(ctx: Context, calls: list[Call], index: int, side: str) -> tuple[Yaml, int]:
    """One predicate (side `where`) or condition (`should`) starting at `calls[index]`."""
    if index >= len(calls):
        message = "not-yet: chain ends without a predicate"
        raise UnportableError(message)
    call = calls[index]
    if call.name in CUSTOM:
        message = "custom-predicate"
        raise UnportableError(message)
    key = lower_first(call.name)
    base, _, is_selector = split_key(key, side)
    concept = ctx.vocabulary.get(base)
    if concept is None or (side == "where" and not concept.in_where):
        message = f"not-yet: {call.name}"
        raise UnportableError(message)
    if is_selector:
        kind = next((k for suffix, k in THAT_KINDS if call.name.endswith(suffix)), None)
        if kind is None or call.args:
            message = f"not-yet: {call.name}"
            raise UnportableError(message)
        inner, after = test_entry(ctx, calls, index + 1, "where")
        return {key: {"kind": kind, "where": inner}}, after
    try:
        value = operand(ctx, concept, call)
    except UnportableError as error:
        # Name the fluent method, so reasons group by what the schema or the tool lacks.
        detail = str(error).removeprefix("not-yet: ")
        if error.named or not str(error).startswith("not-yet: ") or detail.startswith(call.name):
            raise
        message = f"not-yet: {call.name} with {detail}"
        raise UnportableError(message, named=True) from error
    return {key: value}, index + 1


def combine(left: Yaml, op: str, right: Yaml) -> Yaml:
    """Left-associative And / Or: `A.And.B.Or.C` is `any: [all: [A, B], C]`."""
    joined = left.get(op) if isinstance(left, dict) and len(left) == 1 else None
    if isinstance(joined, list):
        return {op: [*joined, right]}
    return {op: [left, right]}


def expression(ctx: Context, calls: list[Call], index: int, side: str) -> tuple[Yaml, int]:
    """A chain of predicates or conditions joined by And/Or (AndShould/OrShould)."""
    joins = (
        {"And": "all", "Or": "any"} if side == "where" else {"AndShould": "all", "OrShould": "any"}
    )
    expr, index = test_entry(ctx, calls, index, side)
    while index < len(calls) and calls[index].name in joins and not calls[index].args:
        op = joins[calls[index].name]
        right, index = test_entry(ctx, calls, index + 1, side)
        expr = combine(expr, op, right)
    return expr, index


def selector(ctx: Context, expr: Expr) -> dict[str, Yaml]:
    """`Classes().That()...` as `{ kind, where }`."""
    root, calls = chain(expr)
    if not isinstance(root, Call) or root.name not in PROVIDERS or root.args:
        message = f"not-yet: selector {render(expr)}"
        raise UnportableError(message)
    out: dict[str, Yaml] = {"kind": PROVIDERS[root.name][0]}
    index = 0
    if calls and calls[0].name == "That" and not calls[0].args:
        where, index = expression(ctx, calls, 1, "where")
        out["where"] = where
    if index != len(calls):
        message = f"not-yet: {calls[index].name} in a selector"
        raise UnportableError(message)
    return out


def rule(ctx: Context, expr: Expr) -> tuple[dict[str, Yaml], str]:
    """A whole rule `Provider().That()...Should()...` as `{ select, should }`, and its provider."""
    root, calls = chain(expr)
    if not isinstance(root, Call) or root.name not in PROVIDERS or root.args:
        message = f"not-yet: rule root {render(root)}"
        raise UnportableError(message)
    split = next((i for i, c in enumerate(calls) if c.name == "Should" and not c.args), None)
    if split is None:
        message = "not-yet: rule without Should()"
        raise UnportableError(message)
    select_expr: Expr = root
    for call in calls[:split]:
        select_expr = Call(select_expr, call.name, call.type_args, call.args)
    select = selector(ctx, select_expr)
    should, index = expression(ctx, calls, split + 1, "should")
    out: dict[str, Yaml] = {"select": select, "should": should}
    for call in calls[index:]:
        if call.name == "Because" and len(call.args) == 1:
            out["because"] = ctx.world.text(call.args[0], ctx.helper, ctx.local)
        elif call.name == "WithoutRequiringPositiveResults" and not call.args:
            out["allowEmpty"] = True
        else:
            message = f"not-yet: {call.name}"
            raise UnportableError(message)
    return out, PROVIDERS[root.name][1]


# ---------------------------------------------------------------------------------------------
# Snapshots


@dataclass
class Block:
    """One snapshot block: the query, per-object results or the exception."""

    query: str
    results: list[tuple[bool, str]]
    exception: str | None


def parse_snapshot(text: str) -> list[Block]:
    """Splits a `.verified.txt` into its blocks."""
    blocks: list[Block] = []
    state = "none"
    for line in text.splitlines():
        if line.startswith("Query: "):
            blocks.append(Block(line[len("Query: ") :], [], None))
            state = "results"
        elif state == "results" and line.startswith("Result: "):
            blocks[-1].results.append((line == "Result: True", ""))
        elif state == "results" and line.startswith("Description: ") and blocks[-1].results:
            passed, _ = blocks[-1].results[-1]
            blocks[-1].results[-1] = (passed, line[len("Description: ") :])
        elif state == "results" and line.startswith("Exception: "):
            blocks[-1].exception = line[len("Exception: ") :]
            state = "none"
        elif state == "results" and line.startswith("Message: "):
            state = "none"
    return blocks


def expectation(block: Block, assert_call: Call, objects_: list[str]) -> dict[str, Yaml]:
    """The expected outcome of a block, checked against the assert kind."""
    if assert_call.name == "AssertException":
        if block.exception is None or len(assert_call.type_args) != 1:
            message = "pairing-mismatch: AssertException without an Exception line"
            raise UnportableError(message)
        return {"error": assert_call.type_args[0].text()}
    if block.exception is not None:
        message = "pairing-mismatch: Exception line for a non-exception assert"
        raise UnportableError(message)
    if len(block.results) == 1 and block.results[0][1].startswith(VACUOUS):
        return {"vacuous": True}
    if len(block.results) == 1 and block.results[0][1] == NO_OBJECTS:
        # "There are no objects matching the criteria": the rule fails with no object to name.
        return {"passes": False}
    passed: set[str] = set()
    failed: set[str] = set()
    for ok, description in block.results:
        name = next(
            (o for o in objects_ if description == o or description.startswith(o + " ")),
            None,
        )
        if name is None:
            message = f"pairing-mismatch: no object prefixes {description[:60]!r}"
            raise UnportableError(message)
        (passed if ok else failed).add(name)
    kind_ok = {
        "AssertNoViolations": not failed,
        "AssertOnlyViolations": not passed,
        "AssertAnyViolations": bool(passed) and bool(failed),
    }
    if not kind_ok.get(assert_call.name, False):
        message = f"pairing-mismatch: results disagree with {assert_call.name}"
        raise UnportableError(message)
    pass_list: list[Yaml] = [*sorted(passed)]
    fail_list: list[Yaml] = [*sorted(failed)]
    return {"pass": pass_list, "fail": fail_list}


# ---------------------------------------------------------------------------------------------
# One test class


@dataclass
class Found:
    """One assert statement: its rule expression, the assert call, locals and helper."""

    expr: Expr | None
    assert_call: Call | None
    local: dict[str, Expr]
    helper: str | None
    problem: str | None


@dataclass
class Output:
    """What the run produces."""

    ported: dict[str, dict[str, Yaml]] = field(default_factory=dict)
    unported: list[dict[str, Yaml]] = field(default_factory=list)
    counts: dict[str, int] = field(default_factory=dict)
    mismatches: list[str] = field(default_factory=list)


def scan(method: Method) -> list[Found]:
    """Every assert statement in a test method, with the locals in force at that point."""
    local: dict[str, Expr] = {}
    helpers: dict[str, str] = {}
    found: list[Found] = []
    for statement in statements(method.body):
        tokens = statement.tokens
        texts = [t.text for t in tokens]
        has_assert = any(t in ASSERTS for t in texts)
        start = 1 if texts[:1] == ["var"] else 0
        is_assign = len(texts) > start + 2 and texts[start + 1] == "=" and not has_assert
        if statement.control and has_assert:
            found.append(Found(None, None, dict(local), None, "not-yet: assert in control flow"))
            continue
        if is_assign and tokens[start].kind == "id":
            try:
                value = parse_expression(tokens[start + 2 :])
            except UnportableError as error:
                value = Opaque(str(error))
            if isinstance(value, New) and value.type.parts[-1].endswith(("Helper", "Helpers")):
                helpers[texts[start]] = value.type.parts[-1]
            else:
                local[texts[start]] = inline(value, local)
            continue
        if has_assert:
            found.append(assert_statement(tokens, dict(local), helpers))
    return found


def assert_statement(tokens: list[Token], local: dict[str, Expr], helpers: dict[str, str]) -> Found:
    """Parses `<rule>.AssertX(helper);`."""
    try:
        expr = parse_expression(tokens)
    except UnportableError as error:
        return Found(None, None, local, None, str(error))
    if not (isinstance(expr, Call) and expr.name in ASSERTS and expr.target is not None):
        return Found(None, None, local, None, "not-yet: assert inside an expression")
    if len(expr.args) != 1 or not isinstance(expr.args[0], Name):
        return Found(None, None, local, None, "not-yet: assert without a helper")
    helper = helpers.get(expr.args[0].ident)
    return Found(expr.target, expr, local, helper, None if helper else "not-yet: unknown helper")


@dataclass
class ClassRun:
    """The inputs for one element test class."""

    source: str
    name: str
    tokens: list[Token]
    snapshots: Path


def port_class(run: ClassRun, world: World, vocabulary: dict[str, Concept], out: Output) -> None:
    """Ports every snapshot test of one class into `out`."""
    namespaces = usings(run.tokens)
    cases: list[dict[str, Yaml]] = []
    architectures: set[tuple[str, ...]] = set()
    for method in test_methods(run.tokens):
        snapshot = run.snapshots / f"{run.name}.{method.name}.verified.txt"
        body = [t.text for t in method.body]
        if "AssertSnapshotMatches" not in body or not snapshot.exists():
            continue
        blocks = parse_snapshot(snapshot.read_text(encoding="utf-8-sig"))
        found = scan(method)
        if len(found) != len(blocks):
            out.mismatches.append(
                f"{run.name}.{method.name}: {len(found)} asserts, {len(blocks)} blocks"
            )
            for number, block in enumerate(blocks, 1):
                out.unported.append(
                    unported(run.source, method.name, number, block.query, "pairing-mismatch")
                )
            continue
        for number, (item, block) in enumerate(zip(found, blocks, strict=True), 1):
            try:
                case, assemblies = port_case(item, block, world, vocabulary, namespaces)
            except UnportableError as error:
                out.unported.append(
                    unported(run.source, method.name, number, block.query, str(error))
                )
                if str(error).startswith("pairing-mismatch"):
                    out.mismatches.append(f"{run.name}.{method.name}#{number}: {error}")
                continue
            case = {"id": f"{method.name}#{number}", **case}
            cases.append(case)
            architectures.add(tuple(assemblies))
            case["architecture"] = list(assemblies)
    if cases:
        out.ported[run.name] = document(run.source, dedupe(cases), architectures)


def port_case(
    item: Found,
    block: Block,
    world: World,
    vocabulary: dict[str, Concept],
    namespaces: list[str],
) -> tuple[dict[str, Yaml], list[str]]:
    """Translates one assert statement and its block into a case."""
    if item.problem is not None or item.expr is None or item.assert_call is None:
        raise UnportableError(item.problem or "not-yet: unparsed")
    if item.helper is None or item.helper not in world.helpers:
        message = f"not-yet: helper {item.helper}"
        raise UnportableError(message)
    helper = world.helpers[item.helper]
    ctx = Context(world, vocabulary, helper, item.local, namespaces)
    expr = inline(item.expr, item.local)
    translated, provider = rule(ctx, expr)
    if not block.query.startswith(provider + " "):
        message = f"pairing-mismatch: query does not start with {provider!r}"
        raise UnportableError(message)
    assemblies = world.assemblies(helper)
    expect = expectation(block, item.assert_call, world.graphs.objects(assemblies))
    case: dict[str, Yaml] = {
        "query": block.query,
        "csharp": render(expr),
        "rule": translated,
        "expect": expect,
    }
    return case, assemblies


def unported(
    source: str, test: str, block: int | None, query: str | None, reason: str
) -> dict[str, Yaml]:
    """One unported.json entry."""
    return {"source": source, "test": test, "block": block, "query": query, "reason": reason}


def dedupe(cases: list[dict[str, Yaml]]) -> list[dict[str, Yaml]]:
    """Folds cases with identical rule, expectation and architecture into the first."""
    kept: dict[str, dict[str, Yaml]] = {}
    for case in cases:
        key = json.dumps([case["rule"], case["expect"], case["architecture"]], sort_keys=True)
        if key in kept:
            first = kept[key]
            also = first.setdefault("alsoCovers", [])
            if isinstance(also, list):
                also.append(case["id"])
        else:
            kept[key] = case
    return list(kept.values())


def document(
    source: str, cases: list[dict[str, Yaml]], architectures: set[tuple[str, ...]]
) -> dict[str, Yaml]:
    """The YAML document for one class; `architecture` is per file when every case shares one."""
    order = ("id", "alsoCovers", "query", "csharp", "architecture", "rule", "expect")
    uniform = len(architectures) == 1
    shaped: list[Yaml] = []
    for case in cases:
        ordered = {k: case[k] for k in order if k in case and not (uniform and k == "architecture")}
        shaped.append(ordered)
    doc: dict[str, Yaml] = {"source": source}
    if uniform:
        doc["architecture"] = list(next(iter(architectures)))
    doc["cases"] = shaped
    return doc


# ---------------------------------------------------------------------------------------------
# The run


def checkout(source: str | None, pin: str, work: Path) -> Path:
    """The ArchUnitNET tree: `--source`, or a shallow clone of the pinned tag."""
    if source is not None:
        path = Path(source).resolve()
        git = shutil.which("git")
        if git is not None and (path / ".git").exists():
            tag = subprocess.run(  # noqa: S603 - fixed argv, no shell
                [git, "-C", str(path), "describe", "--tags", "--exact-match"],
                capture_output=True,
                text=True,
                check=False,
            ).stdout.strip()
            if tag and tag != pin:
                message = f"--source is at {tag}, but PIN is {pin}"
                raise SystemExit(message)
        return path
    git = shutil.which("git")
    if git is None:
        message = "git is required to clone ArchUnitNET; or pass --source"
        raise SystemExit(message)
    target = work / "ArchUnitNET"
    subprocess.run(  # noqa: S603 - fixed argv, no shell
        [
            git,
            "-c",
            "advice.detachedHead=false",
            "clone",
            "--quiet",
            "--depth",
            "1",
            "--branch",
            pin,
            UPSTREAM,
            str(target),
        ],
        check=True,
    )
    return target


def scope_tests(tree: Path) -> Iterator[tuple[str, str, bool]]:
    """Every in-scope test method: (source path, name, has a snapshot)."""
    paths = sorted({p for pattern in SCOPE_GLOBS for p in tree.glob(pattern)})
    for path in paths:
        tokens = tokenize(path.read_text(encoding="utf-8-sig"))
        source = path.relative_to(tree).as_posix()
        for method in test_methods(tokens):
            yield source, method.name, "AssertSnapshotMatches" in [t.text for t in method.body]


def build_world(tree: Path) -> World:
    """Loads the graphs, the static architectures and the helpers."""
    graphs = load_graphs(GATE / "graphs")
    harvest_identities(tree / ELEMENTS_DIR / "Snapshots", graphs)
    architectures = parse_architectures(tree / STATIC_ARCHITECTURES, graphs)
    helpers: dict[str, Helper] = {}
    for path in sorted((tree / HELPER_DIR).glob("*.cs")):
        if path.stem.endswith("Extensions"):
            continue
        helper = parse_helper(path)
        helpers[helper.name] = helper
    return World(graphs, architectures, helpers)


def is_hand_ported(path: Path) -> bool:
    """Whether a ported/*.yaml file was written by hand: its first line says so."""
    first = path.read_text(encoding="utf-8").split("\n", 1)[0]
    return first.startswith("# Ported from ArchUnitNET ") and HAND in first


def hand_ported() -> tuple[int, set[tuple[str, str]]]:
    """The number of hand-ported cases, and the (source, test) pairs they cover."""
    cases = 0
    covered: set[tuple[str, str]] = set()
    for path in sorted((GATE / "ported").glob("*.yaml")):
        if not is_hand_ported(path):
            continue
        doc = yaml.safe_load(path.read_text(encoding="utf-8"))
        for case in doc["cases"]:
            cases += 1
            for ident in [case["id"], *case.get("alsoCovers", [])]:
                covered.add((doc["source"], str(ident).split("#", 1)[0]))
    return cases, covered


def kept_reasons() -> dict[tuple[str, str], str]:
    """The reasons written by hand for whole unported tests in the current unported.json."""
    path = GATE / "unported.json"
    if not path.exists():
        return {}
    entries = json.loads(path.read_text(encoding="utf-8"))["entries"]
    return {
        (e["source"], e["test"]): e["reason"]
        for e in entries
        if e["block"] is None
        and e["reason"] != "custom-predicate"
        and not e["reason"].startswith("not-yet")
    }


def run(tree: Path) -> Output:
    """Ports every element test class and lists what is not ported."""
    hand_cases, covered = hand_ported()
    reasons = kept_reasons()
    world = build_world(tree)
    vocabulary = load_vocabulary(ELEMENTS_RS)
    out = Output()
    snapshot_sources: set[str] = set()
    for path in sorted((tree / ELEMENTS_DIR).glob("*.cs")):
        tokens = tokenize(path.read_text(encoding="utf-8-sig"))
        source = path.relative_to(tree).as_posix()
        port_class(
            ClassRun(source, first_class(tokens), tokens, tree / ELEMENTS_DIR / "Snapshots"),
            world,
            vocabulary,
            out,
        )
    for source, test, snapshot in scope_tests(tree):
        if snapshot:
            snapshot_sources.add(source)
            continue
        if (source, test) in covered:
            continue
        custom = source.endswith("CustomSyntaxElementsTests.cs")
        reason = (
            "custom-predicate" if custom else reasons.get((source, test), "not-yet: non-snapshot")
        )
        out.unported.append(unported(source, test, None, None, reason))
    out.unported.sort(key=lambda e: (str(e["source"]), str(e["test"]), e["block"] or 0))
    cases = hand_cases + sum(
        len(d["cases"]) for d in out.ported.values() if isinstance(d["cases"], list)
    )
    out.counts = {
        "total": cases + len(out.unported),
        "ported": cases,
        "customPredicate": sum(1 for e in out.unported if e["reason"] == "custom-predicate"),
    }
    return out


def dump(data: Yaml) -> str:
    """Stable YAML: insertion order as built, flow style for leaf collections, no wrapping."""
    return str(
        yaml.safe_dump(
            data, sort_keys=False, default_flow_style=None, width=4096, allow_unicode=True
        )
    )


def write(out: Output, pin: str) -> None:
    """Writes ported/*.yaml, unported.json and ported.json."""
    ported_dir = GATE / "ported"
    ported_dir.mkdir(exist_ok=True)
    for existing in ported_dir.glob("*.yaml"):
        if is_hand_ported(existing):
            if existing.stem in out.ported:
                message = f"{existing.name} is hand-ported; the tool would overwrite it"
                raise SystemExit(message)
        elif existing.stem not in out.ported:
            existing.unlink()
    for name, doc in sorted(out.ported.items()):
        header = HEADER.format(pin=pin, source=doc["source"])
        (ported_dir / f"{name}.yaml").write_text(header + dump(doc), encoding="utf-8")
    unported_doc = {"$comment": UNPORTED_COMMENT, "pin": pin, "entries": out.unported}
    (GATE / "unported.json").write_text(json.dumps(unported_doc, indent=2) + "\n", encoding="utf-8")
    ported_doc = {"$comment": PORTED_COMMENT, "pin": pin, **out.counts}
    (GATE / "ported.json").write_text(json.dumps(ported_doc, indent=2) + "\n", encoding="utf-8")


def report(out: Output) -> None:
    """Prints the counts, per class and per reason."""
    lines: list[str] = []
    for name, doc in sorted(out.ported.items()):
        cases = doc["cases"]
        lines.append(f"ported {name}: {len(cases) if isinstance(cases, list) else 0}")
    reasons: dict[str, int] = {}
    for entry in out.unported:
        reasons[str(entry["reason"])] = reasons.get(str(entry["reason"]), 0) + 1
    for reason, count in sorted(reasons.items(), key=lambda r: (-r[1], r[0])):
        lines.append(f"unported {count:4d} {reason}")
    lines.extend(f"mismatch {mismatch}" for mismatch in out.mismatches)
    lines.append(json.dumps(out.counts))
    sys.stdout.write("\n".join(lines) + "\n")


def main(argv: list[str] | None = None) -> int:
    """Entry point."""
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0] if __doc__ else None)
    parser.add_argument("--source", help="an ArchUnitNET checkout at the pinned tag")
    parser.add_argument("--quiet", action="store_true", help="do not print the counts")
    args = parser.parse_args(argv)
    pin = (GATE / "PIN").read_text(encoding="utf-8").strip()
    with tempfile.TemporaryDirectory() as work:
        tree = checkout(args.source, pin, Path(work))
        out = run(tree)
    write(out, pin)
    if not args.quiet:
        report(out)
    return 0


if __name__ == "__main__":
    sys.exit(main())
