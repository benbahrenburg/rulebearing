// The `rulebearing` command of the dotnet tool: runs the packaged binary for this machine
// (src/Launcher.cs; docs/plans/pending/0002-wave-2-dotnet-python-element-rules.md, Step 14).

using System.Runtime.InteropServices;

namespace Rulebearing.Tool;

/// <summary>The tool's entry point.</summary>
internal static class Program
{
    /// <summary>Runs the binary with <paramref name="args"/> and exits with its exit code.</summary>
    /// <param name="args">The command line, passed on unchanged.</param>
    /// <returns>The binary's exit code, or 2 when there is no binary to run.</returns>
    public static int Main(string[] args) => Launcher.Run(
        args,
        Console.Error,
        AppContext.BaseDirectory,
        Launcher.Rid(RuntimeInformation.RuntimeIdentifier, Launcher.CurrentOs(), RuntimeInformation.ProcessArchitecture),
        Environment.GetEnvironmentVariable(Launcher.BinaryVariable));
}
