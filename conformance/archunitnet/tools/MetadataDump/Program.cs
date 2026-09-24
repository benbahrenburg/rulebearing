// Prints table row counts, and for three named methods their signature and the token operands of
// their IL, as System.Reflection.Metadata reads them. The output is pasted into
// crates/rb-extract-dotnet/tests/ecma335.rs as the expected values.
using System.Reflection.Metadata;
using System.Reflection.Metadata.Ecma335;
using System.Reflection.PortableExecutable;

if (args.Length < 1)
{
    Console.Error.WriteLine("usage: MetadataDump <assembly.dll> [Type::Method ...]");
    return 2;
}

using var stream = File.OpenRead(args[0]);
using var pe = new PEReader(stream);
var md = pe.GetMetadataReader();
foreach (var table in new[] { TableIndex.TypeRef, TableIndex.TypeDef, TableIndex.Field, TableIndex.MethodDef,
             TableIndex.MemberRef, TableIndex.CustomAttribute, TableIndex.Property, TableIndex.TypeSpec,
             TableIndex.AssemblyRef, TableIndex.NestedClass, TableIndex.InterfaceImpl })
{
    Console.WriteLine($"rows {table} {md.GetTableRowCount(table)}");
}
var pdbPath = Path.ChangeExtension(args[0], ".pdb");
using var pdbProvider = File.Exists(pdbPath)
    ? MetadataReaderProvider.FromPortablePdbStream(File.OpenRead(pdbPath))
    : null;
var pdb = pdbProvider?.GetMetadataReader();
if (pdb is not null)
{
    Console.WriteLine($"documents {pdb.Documents.Count}");
}
var assembly = md.GetAssemblyDefinition();
Console.WriteLine($"assembly {md.GetString(assembly.Name)} {assembly.Version}");

foreach (var wanted in args.Skip(1))
{
    var (typeName, methodName) = (wanted.Split("::")[0], wanted.Split("::")[1]);
    foreach (var typeHandle in md.TypeDefinitions)
    {
        var type = md.GetTypeDefinition(typeHandle);
        var full = $"{md.GetString(type.Namespace)}.{md.GetString(type.Name)}";
        if (full != typeName)
        {
            continue;
        }
        foreach (var methodHandle in type.GetMethods())
        {
            var method = md.GetMethodDefinition(methodHandle);
            if (md.GetString(method.Name) != methodName)
            {
                continue;
            }
            var row = MetadataTokens.GetRowNumber(methodHandle);
            var sig = md.GetBlobBytes(method.Signature);
            Console.WriteLine($"method {wanted} row {row} sig {Convert.ToHexString(sig)}");
            if (pdb is not null)
            {
                var points = pdb.GetMethodDebugInformation(methodHandle.ToDebugInformationHandle())
                    .GetSequencePoints()
                    .Where(p => !p.IsHidden)
                    .Select(p => $"{p.Offset:X}@{p.StartLine}:{p.StartColumn}");
                Console.WriteLine($"points {wanted} {string.Join(" ", points)}");
            }
            if (method.RelativeVirtualAddress == 0)
            {
                continue;
            }
            var il = pe.GetMethodBody(method.RelativeVirtualAddress).GetILReader();
            var tokens = new List<string>();
            while (il.RemainingBytes > 0)
            {
                var offset = il.Offset;
                int op = il.ReadByte();
                if (op == 0xFE)
                {
                    op = 0xFE00 | il.ReadByte();
                }
                var size = OperandSize(op);
                if (size == -1)
                {
                    var n = il.ReadInt32();
                    il.Offset += 4 * n;
                }
                else if (size == 4 && IsToken(op))
                {
                    tokens.Add($"{offset:X}:{op:X}:{il.ReadInt32():X8}");
                }
                else
                {
                    il.Offset += size;
                }
            }
            Console.WriteLine($"il {wanted} {string.Join(" ", tokens)}");
        }
    }
}
return 0;

static bool IsToken(int op) => op is 0x27 or 0x28 or 0x6F or 0x73 or 0x72 or (>= 0x7B and <= 0x81) or 0x70 or 0x71
    or 0x74 or 0x75 or 0x79 or 0x8C or 0x8D or 0x8F or (>= 0xA3 and <= 0xA5) or 0xC2 or 0xC6 or 0xD0
    or 0xFE06 or 0xFE07 or 0xFE15 or 0xFE16 or 0xFE1C;

static int OperandSize(int op) => op switch
{
    0x45 => -1,
    >= 0x0E and <= 0x13 or 0x1F or (>= 0x2B and <= 0x37) or 0xDE or 0xFE12 or 0xFE19 => 1,
    >= 0xFE09 and <= 0xFE0E => 2,
    0x21 or 0x23 => 8,
    0x20 or 0x22 or (>= 0x38 and <= 0x44) or 0xDD or 0x29 => 4,
    _ when IsToken(op) => 4,
    _ => 0,
};
