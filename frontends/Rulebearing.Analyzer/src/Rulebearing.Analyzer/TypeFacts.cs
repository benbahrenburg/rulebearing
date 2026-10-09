// A type's facts as the compiled-mode extractor records them, computed from Roslyn's symbols.
//
// Plan: docs/plans/pending/0003-wave-3-operations-surface-inner-loop.md, Step 21. Source:
// crates/rb-extract-dotnet/src/codelayer.rs (build_type, kind, base_chain, the interfaces, the
// flags), body.rs (what a method body depends on) and names.rs (how a type is named). The
// analyzer sees source, not IL, so it reads the same facts from the semantic model: a type's full
// name is its metadata name (`Ns.Outer+Inner`, generic arity as `` `1 ``), its flags are the raw
// metadata flags (a static class is abstract and sealed, structs and enums are sealed), and its
// dependencies are every type its declarations and bodies name: base type and interfaces, field,
// property and method signatures, attributes and their `typeof` arguments, and in bodies the
// declaring type of every method, field, property and event used, every type created, cast to,
// tested for or declared as a local, lambdas and local functions included. A generic type is
// named by its definition, its arguments as dependencies of their own; arrays, pointers and
// by-reference types stand for their element type; type parameters are no dependency.

using Microsoft.CodeAnalysis;
using Microsoft.CodeAnalysis.Operations;

namespace Rulebearing.Analyzer;

/// <summary>The facts one type has for the element rules.</summary>
internal sealed class TypeFacts
{
    /// <summary>The metadata full name, <c>Ns.Outer+Inner`1</c>.</summary>
    public required string FullName { get; init; }

    /// <summary>The metadata name, arity included, no outer type.</summary>
    public required string Name { get; init; }

    /// <summary>The namespace, the outermost type's for a nested type; empty for the global namespace.</summary>
    public required string Namespace { get; init; }

    /// <summary><c>interface</c>, <c>enum</c>, <c>struct</c>, <c>attribute</c> or <c>class</c>.</summary>
    public required string Kind { get; init; }

    /// <summary><c>public</c>, <c>internal</c>, <c>private</c>, <c>protected</c>, <c>protected-internal</c> or <c>private-protected</c>.</summary>
    public required string Visibility { get; init; }

    /// <summary>The raw abstract flag, false for an interface.</summary>
    public bool Abstract { get; init; }

    /// <summary>The raw sealed flag.</summary>
    public bool Sealed { get; init; }

    /// <summary>Abstract and sealed: a static class.</summary>
    public bool Static { get; init; }

    /// <summary>A record class.</summary>
    public bool Record { get; init; }

    /// <summary>Whether the type is nested.</summary>
    public bool Nested { get; init; }

    /// <summary>Every instance field and property is read-only, init setters allowed.</summary>
    public bool Immutable { get; init; }

    /// <summary>The base chain, nearest first, while the base is defined in this compilation, plus the first base outside it.</summary>
    public required IReadOnlyList<string> BaseTypes { get; init; }

    /// <summary>The interfaces the type and its bases in this compilation implement.</summary>
    public required IReadOnlyList<string> Interfaces { get; init; }

    /// <summary>The simple assembly name.</summary>
    public required string Assembly { get; init; }

    /// <summary>The assembly's display name.</summary>
    public required string AssemblyFullName { get; init; }

    /// <summary>Every type the type depends on, by full name.</summary>
    public required IReadOnlyCollection<string> Dependencies { get; init; }

    /// <summary>Where each dependency is first named in source, when it is.</summary>
    public IReadOnlyDictionary<string, Location> DependencyLocations { get; init; } = new Dictionary<string, Location>();

    /// <summary>A type of another assembly: a dependency target, never selected unless asked.</summary>
    public bool Referenced { get; init; }

    /// <summary>Where the type is reported: the declaration in a file someone wrote.</summary>
    public Location? Location { get; init; }

    /// <summary>The source file the type is attributed to, as written in the compilation.</summary>
    public string? FilePath { get; init; }
}

/// <summary>Builds <see cref="TypeFacts"/> from symbols.</summary>
internal static class Facts
{
    /// <summary>The metadata full name of a type definition: <c>Ns.Outer+Inner`1</c>.</summary>
    public static string FullName(INamedTypeSymbol type)
    {
        var definition = type.OriginalDefinition;
        if (definition.ContainingType is { } outer)
        {
            return FullName(outer) + "+" + definition.MetadataName;
        }
        var ns = Namespace(definition);
        return ns.Length == 0 ? definition.MetadataName : ns + "." + definition.MetadataName;
    }

    /// <summary>The outermost type's namespace, empty for the global namespace.</summary>
    public static string Namespace(INamedTypeSymbol type)
    {
        var outer = type;
        while (outer.ContainingType is { } containing)
        {
            outer = containing;
        }
        return outer.ContainingNamespace is { IsGlobalNamespace: false } ns ? ns.ToDisplayString() : string.Empty;
    }

    /// <summary>The extractor's kind: interface, enum, struct, attribute or class (delegates and records are classes).</summary>
    public static string Kind(INamedTypeSymbol type)
    {
        switch (type.TypeKind)
        {
            case TypeKind.Interface:
                return "interface";
            case TypeKind.Enum:
                return "enum";
            case TypeKind.Struct:
                return "struct";
            default:
                break;
        }
        for (var b = type.BaseType; b is not null; b = b.BaseType)
        {
            if (FullName(b) == "System.Attribute")
            {
                return "attribute";
            }
        }
        return "class";
    }

    /// <summary>The metadata visibility of a type, as the extractor names it.</summary>
    public static string Visibility(INamedTypeSymbol type) => type.DeclaredAccessibility switch
    {
        Accessibility.Public => "public",
        Accessibility.Private => "private",
        Accessibility.Protected => "protected",
        Accessibility.ProtectedAndInternal => "private-protected",
        Accessibility.ProtectedOrInternal => "protected-internal",
        _ => "internal",
    };

    /// <summary>The facts of <paramref name="type"/>; <paramref name="local"/> says whether it is defined in the compilation being analysed. Every dependency's symbol is added to <paramref name="targets"/>.</summary>
    public static TypeFacts Of(INamedTypeSymbol type, Compilation compilation, bool local, IDictionary<string, INamedTypeSymbol> targets)
    {
        var kind = Kind(type);
        var isClass = kind is "class" or "attribute";
        var isStaticClass = type.IsStatic && type.TypeKind == TypeKind.Class;
        var sealedFlag = type.IsSealed || isStaticClass || type.TypeKind is TypeKind.Struct or TypeKind.Enum or TypeKind.Delegate;
        var abstractFlag = (type.IsAbstract || isStaticClass) && type.TypeKind != TypeKind.Interface;
        var assembly = type.ContainingAssembly;
        var (location, file) = local ? Attribution(type) : (null, null);
        var collector = new Collector(targets);
        if (local)
        {
            Collect(type, compilation, collector);
        }
        return new TypeFacts
        {
            FullName = FullName(type),
            Name = type.OriginalDefinition.MetadataName,
            Namespace = Namespace(type),
            Kind = kind,
            Visibility = Visibility(type),
            Abstract = abstractFlag,
            Sealed = sealedFlag,
            Static = abstractFlag && sealedFlag && isClass,
            Record = isClass && type.IsRecord,
            Nested = type.ContainingType is not null,
            Immutable = Immutable(type),
            BaseTypes = local ? BaseChain(type, compilation) : [],
            Interfaces = local ? Interfaces(type, compilation) : [],
            Assembly = assembly?.Identity.Name ?? string.Empty,
            AssemblyFullName = assembly?.Identity.GetDisplayName() ?? string.Empty,
            Dependencies = collector.Found,
            DependencyLocations = collector.Where,
            Referenced = !local,
            Location = location,
            FilePath = file,
        };
    }

    /// <summary>The file a type is attributed to: a declaration outside build output when there is one (ADR-0061), else the first.</summary>
    public static (Location?, string?) Attribution(INamedTypeSymbol type)
    {
        var locations = type.Locations.Where(l => l.IsInSource).ToList();
        var written = locations.FirstOrDefault(l => !IsBuildOutput(l.SourceTree?.FilePath ?? string.Empty)) ?? locations.FirstOrDefault();
        return (written, written?.SourceTree?.FilePath);
    }

    /// <summary>Whether a path passes through a folder the extractor treats as build output.</summary>
    public static bool IsBuildOutput(string path) =>
        path.Split('/', '\\').Any(segment => segment is "bin" or "obj" or "node_modules" or ".git" or ".vs" or "artifacts");

    private static bool InCompilation(INamedTypeSymbol type, Compilation compilation) =>
        SymbolEqualityComparer.Default.Equals(type.ContainingAssembly, compilation.Assembly);

    private static List<string> BaseChain(INamedTypeSymbol type, Compilation compilation)
    {
        var chain = new List<string>();
        for (var b = type.BaseType; b is not null; b = b.BaseType)
        {
            chain.Add(FullName(b));
            if (!InCompilation(b, compilation))
            {
                break;
            }
        }
        return chain;
    }

    private static List<string> Interfaces(INamedTypeSymbol type, Compilation compilation)
    {
        var names = new List<string>();
        void Add(INamedTypeSymbol owner)
        {
            // The compiler writes every interface a type implements, the base interfaces of the
            // ones it declares included, on the type itself.
            foreach (var i in owner.Interfaces.SelectMany(i => new[] { i }.Concat(i.AllInterfaces)))
            {
                var name = FullName(i);
                if (!names.Contains(name))
                {
                    names.Add(name);
                }
            }
        }
        Add(type);
        for (var b = type.BaseType; b is not null && InCompilation(b, compilation); b = b.BaseType)
        {
            Add(b);
        }
        return names;
    }

    private static bool Immutable(INamedTypeSymbol type)
    {
        foreach (var member in type.GetMembers())
        {
            switch (member)
            {
                case IFieldSymbol { IsStatic: false, IsConst: false } field when !field.IsImplicitlyDeclared:
                    if (!field.IsReadOnly)
                    {
                        return false;
                    }
                    break;
                case IPropertySymbol { IsStatic: false } property:
                    if (property.SetMethod is { IsInitOnly: false })
                    {
                        return false;
                    }
                    break;
                case IEventSymbol { IsStatic: false } e when !e.IsAbstract && e.AddMethod?.IsImplicitlyDeclared != false:
                    // A field-like event is a mutable backing field.
                    return false;
                default:
                    break;
            }
        }
        return true;
    }

    /// <summary>Collects every type <paramref name="type"/>'s declarations and bodies name.</summary>
    private static void Collect(INamedTypeSymbol type, Compilation compilation, Collector collector)
    {
        collector.At = Attribution(type).Item1;
        if (type.BaseType is { } baseType)
        {
            collector.Type(baseType);
        }
        var local = new HashSet<string>(Interfaces(type, compilation), StringComparer.Ordinal);
        foreach (var i in type.AllInterfaces.Where(i => local.Contains(FullName(i))))
        {
            collector.Type(i);
        }
        foreach (var parameter in type.TypeParameters)
        {
            foreach (var constraint in parameter.ConstraintTypes)
            {
                collector.Type(constraint);
            }
            if (parameter.HasValueTypeConstraint || parameter.HasUnmanagedTypeConstraint)
            {
                collector.Type(compilation.GetSpecialType(SpecialType.System_ValueType));
            }
        }
        collector.Attributes(type.GetAttributes());
        if (type.TypeKind == TypeKind.Enum && type.EnumUnderlyingType is { } underlying)
        {
            collector.Type(underlying);
            collector.Type(type);
        }
        foreach (var member in type.GetMembers())
        {
            collector.Member(member);
        }
        foreach (var reference in type.DeclaringSyntaxReferences)
        {
            var syntax = reference.GetSyntax();
            var model = compilation.GetSemanticModel(syntax.SyntaxTree);
            foreach (var node in syntax.DescendantNodes(n => n == syntax || !IsTypeDeclaration(n)))
            {
                if (model.GetOperation(node) is { Parent: null } root)
                {
                    collector.Operations(root);
                }
            }
        }
    }

    private static bool IsTypeDeclaration(SyntaxNode node) =>
        node is Microsoft.CodeAnalysis.CSharp.Syntax.BaseTypeDeclarationSyntax or Microsoft.CodeAnalysis.CSharp.Syntax.DelegateDeclarationSyntax;

    /// <summary>Adds the types named by symbols and operations to a set.</summary>
    private sealed class Collector(IDictionary<string, INamedTypeSymbol> targets)
    {
        /// <summary>The names found.</summary>
        public HashSet<string> Found { get; } = new(StringComparer.Ordinal);

        /// <summary>Where each name was first found.</summary>
        public Dictionary<string, Location> Where { get; } = new(StringComparer.Ordinal);

        /// <summary>The location the names found now are recorded at.</summary>
        public Location? At { get; set; }

        private void Add(INamedTypeSymbol named)
        {
            var name = FullName(named);
            if (Found.Add(name) && At is { IsInSource: true } at)
            {
                Where[name] = at;
            }
            if (!targets.ContainsKey(name))
            {
                targets[name] = named.OriginalDefinition;
            }
        }

        public void Type(ITypeSymbol? type)
        {
            switch (type)
            {
                case null:
                case ITypeParameterSymbol:
                    return;
                case IArrayTypeSymbol array:
                    Type(array.ElementType);
                    return;
                case IPointerTypeSymbol pointer:
                    Type(pointer.PointedAtType);
                    return;
                case IFunctionPointerTypeSymbol:
                    return;
                case IDynamicTypeSymbol:
                    return;
                case INamedTypeSymbol named:
                    if (named.TypeKind == TypeKind.Error || named.MetadataName.StartsWith("<", StringComparison.Ordinal))
                    {
                        Arguments(named);
                        return;
                    }
                    Add(named);
                    Arguments(named);
                    return;
                default:
                    return;
            }
        }

        public void Arguments(INamedTypeSymbol named)
        {
            for (var t = named; t is not null; t = t.ContainingType)
            {
                foreach (var argument in t.TypeArguments)
                {
                    Type(argument);
                }
            }
        }

        public void Attributes(IEnumerable<AttributeData> attributes)
        {
            foreach (var attribute in attributes)
            {
                Type(attribute.AttributeClass);
                foreach (var argument in attribute.ConstructorArguments.Concat(attribute.NamedArguments.Select(n => n.Value)))
                {
                    TypeOf(argument);
                }
            }
        }

        private void TypeOf(TypedConstant constant)
        {
            if (constant.Kind == TypedConstantKind.Array)
            {
                foreach (var item in constant.Values)
                {
                    TypeOf(item);
                }
            }
            else if (constant.Kind == TypedConstantKind.Type)
            {
                Type(constant.Value as ITypeSymbol);
            }
        }

        public void Member(ISymbol member)
        {
            // A nested type is an element of its own; its dependencies are not the outer type's.
            if (member is INamedTypeSymbol || member.Name.StartsWith("<", StringComparison.Ordinal))
            {
                return;
            }
            if (member.Locations.FirstOrDefault(l => l.IsInSource) is { } declared)
            {
                At = declared;
            }
            Attributes(member.GetAttributes());
            switch (member)
            {
                case IFieldSymbol field:
                    Type(field.Type);
                    break;
                case IPropertySymbol property:
                    Type(property.Type);
                    foreach (var accessor in new[] { property.GetMethod, property.SetMethod })
                    {
                        if (accessor is not null)
                        {
                            Method(accessor);
                        }
                    }
                    break;
                case IEventSymbol e:
                    Type(e.Type);
                    foreach (var accessor in new[] { e.AddMethod, e.RemoveMethod })
                    {
                        if (accessor is not null)
                        {
                            Method(accessor);
                        }
                    }
                    break;
                case IMethodSymbol method:
                    Method(method);
                    break;
                default:
                    break;
            }
        }

        private void Method(IMethodSymbol method)
        {
            Attributes(method.GetAttributes());
            Attributes(method.GetReturnTypeAttributes());
            if (method.ReturnType.SpecialType != SpecialType.System_Void)
            {
                Type(method.ReturnType);
            }
            foreach (var parameter in method.Parameters)
            {
                Type(parameter.Type);
                Attributes(parameter.GetAttributes());
            }
            foreach (var parameter in method.TypeParameters)
            {
                foreach (var constraint in parameter.ConstraintTypes)
                {
                    Type(constraint);
                }
            }
        }

        private void Declaring(ISymbol? symbol)
        {
            if (symbol?.ContainingType is { } owner)
            {
                Type(owner);
            }
        }

        public void Operations(IOperation root)
        {
            foreach (var operation in new[] { root }.Concat(root.Descendants()))
            {
                At = operation.Syntax.GetLocation();
                switch (operation)
                {
                    case IInvocationOperation invocation:
                        Declaring(invocation.TargetMethod);
                        foreach (var argument in invocation.TargetMethod.TypeArguments)
                        {
                            Type(argument);
                        }
                        break;
                    case IObjectCreationOperation creation:
                        Type(creation.Type);
                        break;
                    case IFieldReferenceOperation field:
                        Declaring(field.Field);
                        break;
                    case IPropertyReferenceOperation property:
                        Declaring(property.Property);
                        break;
                    case IEventReferenceOperation e:
                        Declaring(e.Event);
                        break;
                    case IMethodReferenceOperation reference:
                        Declaring(reference.Method);
                        break;
                    case IDelegateCreationOperation creation:
                        Type(creation.Type);
                        break;
                    case ITypeOfOperation typeOf:
                        Type(typeOf.TypeOperand);
                        break;
                    case IConversionOperation { IsImplicit: false } conversion:
                        Type(conversion.Type);
                        break;
                    case IIsTypeOperation isType:
                        Type(isType.TypeOperand);
                        break;
                    case IDeclarationPatternOperation declaration:
                        Type(declaration.MatchedType);
                        break;
                    case ITypePatternOperation pattern:
                        Type(pattern.MatchedType);
                        break;
                    case IVariableDeclaratorOperation variable:
                        Type(variable.Symbol.Type);
                        break;
                    case IArrayCreationOperation array:
                        Type(array.Type);
                        break;
                    case IDefaultValueOperation defaultValue:
                        Type(defaultValue.Type);
                        break;
                    default:
                        break;
                }
            }
        }
    }
}
