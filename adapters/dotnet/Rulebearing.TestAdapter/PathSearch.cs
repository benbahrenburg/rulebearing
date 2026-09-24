// Where the configuration, graph, saved result and binary are: a relative path is looked up from
// the test assembly's directory upwards, then from the current directory upwards; the binary from
// the option, then RULEBEARING_BINARY, then PATH
// (docs/plans/pending/0002-wave-2-dotnet-python-element-rules.md, Step 14).

namespace Rulebearing.TestAdapter;

/// <summary>Resolves the files and the binary a run needs.</summary>
internal static class PathSearch
{
    /// <summary>The directories a relative path is looked up from, in order.</summary>
    internal static IEnumerable<string> Roots() => [AppContext.BaseDirectory, Environment.CurrentDirectory];

    /// <summary>The full path of an existing file, looked up upwards when relative.</summary>
    public static string File(string path, string what) =>
        Find(path, System.IO.File.Exists)
        ?? throw new RulebearingException(
            $"The {what} file `{path}` was not found in {string.Join(" or ", Roots())} or any directory above them. Fix: give a path relative to a directory above the test project, or an absolute path.");

    /// <summary>The full path of an existing directory, looked up upwards when relative.</summary>
    public static string Directory(string path) =>
        Find(path, System.IO.Directory.Exists)
        ?? throw new RulebearingException(
            $"The working directory `{path}` was not found in {string.Join(" or ", Roots())} or any directory above them. Fix: give a path relative to a directory above the test project, or an absolute path.");

    /// <summary>The binary: the option, else <c>RULEBEARING_BINARY</c>, else <c>rulebearing</c> on <c>PATH</c>.</summary>
    public static string Binary(string? option)
    {
        string? named = option ?? NonEmpty(Environment.GetEnvironmentVariable(RulebearingOptions.BinaryVariable));
        if (named is not null)
        {
            bool isPath = Path.IsPathRooted(named)
                || named.Contains(Path.DirectorySeparatorChar, StringComparison.Ordinal)
                || named.Contains(Path.AltDirectorySeparatorChar, StringComparison.Ordinal);
            string? found = isPath ? Find(named, System.IO.File.Exists) : OnPath(named);
            return found ?? throw new RulebearingException(
                $"The rulebearing binary `{named}` does not exist. Fix: point {(option is null ? RulebearingOptions.BinaryVariable : "the Binary option")} at the binary, or install it with `dotnet tool install -g Rulebearing`.");
        }

        return OnPath("rulebearing") ?? throw new RulebearingException(
            $"No rulebearing binary: {RulebearingOptions.BinaryVariable} is not set and `rulebearing` is not on PATH. Fix: install it with `dotnet tool install -g Rulebearing`, or set {RulebearingOptions.BinaryVariable} to the binary.");
    }

    /// <summary>The first file called <paramref name="name"/> (with a Windows executable extension) on <c>PATH</c>.</summary>
    internal static string? OnPath(string name)
    {
        string[] extensions = OperatingSystem.IsWindows()
            ? [string.Empty, .. (NonEmpty(Environment.GetEnvironmentVariable("PATHEXT")) ?? ".EXE;.CMD;.BAT").Split(';', StringSplitOptions.RemoveEmptyEntries)]
            : [string.Empty];
        string path = Environment.GetEnvironmentVariable("PATH") ?? string.Empty;
        foreach (string directory in path.Split(Path.PathSeparator, StringSplitOptions.RemoveEmptyEntries))
        {
            foreach (string extension in extensions)
            {
                string candidate = Path.Combine(directory, name + extension);
                if (System.IO.File.Exists(candidate))
                {
                    return Path.GetFullPath(candidate);
                }
            }
        }

        return null;
    }

    private static string? Find(string path, Func<string, bool> exists)
    {
        if (Path.IsPathRooted(path))
        {
            return exists(path) ? Path.GetFullPath(path) : null;
        }

        foreach (string root in Roots())
        {
            for (DirectoryInfo? directory = new(root); directory is not null; directory = directory.Parent)
            {
                string candidate = Path.Combine(directory.FullName, path);
                if (exists(candidate))
                {
                    return Path.GetFullPath(candidate);
                }
            }
        }

        return null;
    }

    private static string? NonEmpty(string? value) => string.IsNullOrWhiteSpace(value) ? null : value;
}
