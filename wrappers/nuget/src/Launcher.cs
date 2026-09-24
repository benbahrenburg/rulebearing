// The `rulebearing` dotnet tool's launcher: picks the binary for this machine's runtime identifier
// from runtimes/<rid>/native/ beside the tool, runs it with every argument unchanged and the
// console inherited, and exits with its exit code. It never re-implements a subcommand
// (CLAUDE.md, wrappers; docs/architecture.md#distribution; FR-DIST-01).
// Plan: docs/plans/pending/0002-wave-2-dotnet-python-element-rules.md, Step 14 (2H).
// The six targets are the matrix of .github/workflows/release.yml, as the npm wrapper's
// wrappers/npm/src/platforms.ts lists them.

using System.Diagnostics;
using System.Runtime.InteropServices;
using System.Runtime.Versioning;

namespace Rulebearing.Tool;

/// <summary>Finds and runs the platform binary.</summary>
internal static class Launcher
{
    /// <summary>The environment variable naming a binary to run instead of the packaged one.</summary>
    public const string BinaryVariable = "RULEBEARING_BINARY";

    /// <summary>The exit code for a run that cannot start (ADR-0008: the run cannot be trusted).</summary>
    public const int CannotRun = 2;

    /// <summary>Each packaged runtime identifier and the release target whose binary it carries.</summary>
    public static readonly IReadOnlyDictionary<string, string> Targets = new Dictionary<string, string>(StringComparer.Ordinal)
    {
        ["linux-x64"] = "x86_64-unknown-linux-gnu",
        ["linux-musl-x64"] = "x86_64-unknown-linux-musl",
        ["linux-arm64"] = "aarch64-unknown-linux-gnu",
        ["osx-arm64"] = "aarch64-apple-darwin",
        ["osx-x64"] = "x86_64-apple-darwin",
        ["win-x64"] = "x86_64-pc-windows-msvc",
    };

    /// <summary>
    /// The packaged runtime identifier for a machine. The runtime's own identifier says musl on a
    /// musl build of .NET (Alpine); otherwise the operating system and architecture decide.
    /// </summary>
    /// <param name="runtimeIdentifier"><see cref="RuntimeInformation.RuntimeIdentifier"/>.</param>
    /// <param name="os"><c>linux</c>, <c>osx</c> or <c>win</c>; anything else has no binary.</param>
    /// <param name="architecture">The process architecture.</param>
    /// <returns>The identifier, which <see cref="Targets"/> may not carry.</returns>
    public static string Rid(string runtimeIdentifier, string os, Architecture architecture)
    {
        ArgumentNullException.ThrowIfNull(runtimeIdentifier);
        string arch = architecture switch
        {
            Architecture.X64 => "x64",
            Architecture.Arm64 => "arm64",
            Architecture.X86 => "x86",
            Architecture.Arm => "arm",
            _ => architecture.ToString().ToLowerInvariant(),
        };
        bool musl = os == "linux" && runtimeIdentifier.StartsWith("linux-musl-", StringComparison.Ordinal);
        return musl ? $"linux-musl-{arch}" : $"{os}-{arch}";
    }

    /// <summary>This machine's operating system, as runtime identifiers spell it.</summary>
    /// <returns><c>win</c>, <c>osx</c>, <c>linux</c>, or the platform's own description.</returns>
    public static string CurrentOs() =>
        OperatingSystem.IsWindows() ? "win"
        : OperatingSystem.IsMacOS() ? "osx"
        : OperatingSystem.IsLinux() ? "linux"
        : RuntimeInformation.OSDescription;

    /// <summary>The binary's file name for a runtime identifier.</summary>
    /// <param name="rid">The runtime identifier.</param>
    /// <returns><c>rulebearing.exe</c> on Windows, <c>rulebearing</c> elsewhere.</returns>
    public static string Executable(string rid) =>
        rid.StartsWith("win-", StringComparison.Ordinal) ? "rulebearing.exe" : "rulebearing";

    /// <summary>Where the binary to run is, or why there is none.</summary>
    /// <param name="baseDirectory">The tool's own directory.</param>
    /// <param name="rid">This machine's runtime identifier.</param>
    /// <param name="overrideBinary">The value of <see cref="BinaryVariable"/>, when set.</param>
    /// <returns>The binary's path, or the one-line reason there is none.</returns>
    public static (string? Binary, string? Problem) Resolve(string baseDirectory, string rid, string? overrideBinary)
    {
        if (!string.IsNullOrWhiteSpace(overrideBinary))
        {
            return File.Exists(overrideBinary)
                ? (Path.GetFullPath(overrideBinary), null)
                : (null, $"rulebearing: {BinaryVariable} names {overrideBinary}, which does not exist. Fix: point it at a rulebearing binary, or unset it to use the packaged one.");
        }

        if (!Targets.TryGetValue(rid, out string? target))
        {
            return (null, $"rulebearing: no packaged binary for {rid}; the package carries {string.Join(", ", Targets.Keys)}. Fix: build one with `cargo build --release -p rb-cli` and set {BinaryVariable} to it.");
        }

        string binary = Path.Combine(baseDirectory, "runtimes", rid, "native", Executable(rid));
        return File.Exists(binary)
            ? (binary, null)
            : (null, $"rulebearing: the {rid} binary ({target}) is missing from {binary}. Fix: reinstall with `dotnet tool update -g Rulebearing`, or set {BinaryVariable} to a rulebearing binary.");
    }

    /// <summary>
    /// Makes a Unix binary executable: NuGet does not keep file modes when it extracts a package.
    /// </summary>
    /// <param name="binary">The binary.</param>
    [UnsupportedOSPlatform("windows")]
    public static void EnsureExecutable(string binary)
    {
        UnixFileMode mode = File.GetUnixFileMode(binary);
        const UnixFileMode execute = UnixFileMode.UserExecute | UnixFileMode.GroupExecute | UnixFileMode.OtherExecute;
        if ((mode & UnixFileMode.UserExecute) == 0)
        {
            File.SetUnixFileMode(binary, mode | execute);
        }
    }

    /// <summary>Runs <paramref name="binary"/> with <paramref name="args"/> and the console inherited.</summary>
    /// <param name="binary">The binary.</param>
    /// <param name="args">The arguments, unchanged.</param>
    /// <returns>The binary's exit code.</returns>
    public static int Execute(string binary, IReadOnlyList<string> args)
    {
        ArgumentNullException.ThrowIfNull(args);
        if (!OperatingSystem.IsWindows())
        {
            EnsureExecutable(binary);
        }

        ProcessStartInfo start = new(binary) { UseShellExecute = false };
        foreach (string arg in args)
        {
            start.ArgumentList.Add(arg);
        }

        // Ctrl+C reaches the binary through the shared console; the launcher waits for it to stop.
        ConsoleCancelEventHandler keepWaiting = static (_, e) => e.Cancel = true;
        Console.CancelKeyPress += keepWaiting;
        try
        {
            using Process process = Process.Start(start)
                ?? throw new InvalidOperationException($"rulebearing: {binary} did not start.");
            process.WaitForExit();
            return process.ExitCode;
        }
        finally
        {
            Console.CancelKeyPress -= keepWaiting;
        }
    }

    /// <summary>The tool's entry point, with its environment passed in.</summary>
    /// <param name="args">The command line.</param>
    /// <param name="error">Where the one-line reason goes when nothing can run.</param>
    /// <param name="baseDirectory">The tool's own directory.</param>
    /// <param name="rid">This machine's runtime identifier.</param>
    /// <param name="overrideBinary">The value of <see cref="BinaryVariable"/>.</param>
    /// <returns>The binary's exit code, or <see cref="CannotRun"/>.</returns>
    public static int Run(IReadOnlyList<string> args, TextWriter error, string baseDirectory, string rid, string? overrideBinary)
    {
        ArgumentNullException.ThrowIfNull(error);
        (string? binary, string? problem) = Resolve(baseDirectory, rid, overrideBinary);
        if (binary is null)
        {
            error.WriteLine(problem);
            return CannotRun;
        }

        try
        {
            return Execute(binary, args);
        }
        catch (Exception e) when (e is System.ComponentModel.Win32Exception or IOException or UnauthorizedAccessException or InvalidOperationException)
        {
            error.WriteLine($"rulebearing: could not run {binary}: {e.Message}. Fix: reinstall with `dotnet tool update -g Rulebearing`, or set {BinaryVariable} to a rulebearing binary.");
            return CannotRun;
        }
    }
}
