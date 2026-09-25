// The launcher: which binary it picks, what it says when there is none, and that it runs the
// binary with the arguments unchanged and passes the exit code through
// (docs/plans/pending/0002-wave-2-dotnet-python-element-rules.md, Step 14).

using System.Runtime.InteropServices;
using System.Runtime.Versioning;
using Xunit;

namespace Rulebearing.Tool.Tests;

/// <summary>The launcher of the <c>Rulebearing</c> dotnet tool.</summary>
public sealed class LauncherTests : IDisposable
{
    private readonly string directory = Path.Combine(Path.GetTempPath(), $"rb-tool-{Guid.NewGuid():N}");

    /// <summary>Creates the temporary tool directory.</summary>
    public LauncherTests() => Directory.CreateDirectory(directory);

    /// <summary>Every packaged identifier is one of the six release targets, in release.yml's order.</summary>
    [Fact]
    public void SixTargets() => Assert.Equal(
        ["x86_64-unknown-linux-gnu", "x86_64-unknown-linux-musl", "aarch64-unknown-linux-gnu", "aarch64-apple-darwin", "x86_64-apple-darwin", "x86_64-pc-windows-msvc"],
        Launcher.Targets.Values);

    /// <summary>The runtime identifier for each machine.</summary>
    /// <param name="runtime">The runtime's own identifier.</param>
    /// <param name="os">The operating system.</param>
    /// <param name="architecture">The architecture.</param>
    /// <param name="expected">The identifier the launcher looks for.</param>
    [Theory]
    [InlineData("linux-x64", "linux", Architecture.X64, "linux-x64")]
    [InlineData("linux-musl-x64", "linux", Architecture.X64, "linux-musl-x64")]
    [InlineData("linux-arm64", "linux", Architecture.Arm64, "linux-arm64")]
    [InlineData("osx-arm64", "osx", Architecture.Arm64, "osx-arm64")]
    [InlineData("osx-x64", "osx", Architecture.X64, "osx-x64")]
    [InlineData("win-x64", "win", Architecture.X64, "win-x64")]
    [InlineData("win-x86", "win", Architecture.X86, "win-x86")]
    [InlineData("linux-arm", "linux", Architecture.Arm, "linux-arm")]
    [InlineData("linux-musl-x64", "osx", Architecture.X64, "osx-x64")]
    [InlineData("freebsd-x64", "freebsd", Architecture.S390x, "freebsd-s390x")]
    public void TheRidForAMachine(string runtime, string os, Architecture architecture, string expected) =>
        Assert.Equal(expected, Launcher.Rid(runtime, os, architecture));

    /// <summary>This machine has an operating system the launcher names.</summary>
    [Fact]
    public void TheCurrentOperatingSystem() => Assert.Matches("^(win|osx|linux)$", Launcher.CurrentOs());

    /// <summary>Only Windows binaries end in .exe.</summary>
    [Fact]
    public void TheExecutableName()
    {
        Assert.Equal("rulebearing.exe", Launcher.Executable("win-x64"));
        Assert.Equal("rulebearing", Launcher.Executable("linux-musl-x64"));
    }

    /// <summary>The packaged binary under runtimes/&lt;rid&gt;/native, or a reason naming the fix.</summary>
    [Fact]
    public void ResolvesThePackagedBinary()
    {
        string binary = Stage("linux-x64", "rulebearing", "exit 0");
        Assert.Equal((binary, null), Launcher.Resolve(directory, "linux-x64", null));
        (string? none, string? missing) = Launcher.Resolve(directory, "osx-arm64", null);
        Assert.Null(none);
        Assert.Contains("aarch64-apple-darwin", missing, StringComparison.Ordinal);
        Assert.Contains("dotnet tool update -g Rulebearing", missing, StringComparison.Ordinal);
        (_, string? unknown) = Launcher.Resolve(directory, "freebsd-x64", null);
        Assert.Contains("no packaged binary for freebsd-x64", unknown, StringComparison.Ordinal);
        Assert.Contains(Launcher.BinaryVariable, unknown, StringComparison.Ordinal);
    }

    /// <summary>RULEBEARING_BINARY wins over the packaged binary, and a wrong one is named.</summary>
    [Fact]
    public void TheVariableWins()
    {
        string binary = Stage("custom", "rulebearing", "exit 0");
        Assert.Equal((binary, null), Launcher.Resolve(directory, "freebsd-x64", binary));
        (string? none, string? problem) = Launcher.Resolve(directory, "linux-x64", "/no/such/rulebearing");
        Assert.Null(none);
        Assert.Contains("/no/such/rulebearing", problem, StringComparison.Ordinal);
        Assert.Equal((null, Launcher.Resolve(directory, "freebsd-x64", null).Problem), Launcher.Resolve(directory, "freebsd-x64", "  "));
    }

    /// <summary>With no binary, one line on stderr and exit 2.</summary>
    [Fact]
    public void NoBinaryIsExitTwo()
    {
        using StringWriter error = new();
        Assert.Equal(Launcher.CannotRun, Launcher.Run(["--version"], error, directory, "osx-arm64", null));
        Assert.StartsWith("rulebearing: the osx-arm64 binary", error.ToString(), StringComparison.Ordinal);
        Assert.Throws<ArgumentNullException>(() => Launcher.Run([], null!, directory, "osx-arm64", null));
    }

    /// <summary>The binary runs with every argument unchanged, and its exit code passes through.</summary>
    [Fact]
    [UnsupportedOSPlatform("windows")]
    public void RunsTheBinaryAndPassesTheExitCode()
    {
        string record = Path.Combine(directory, "args.txt");
        string rid = Launcher.Rid(RuntimeInformation.RuntimeIdentifier, Launcher.CurrentOs(), RuntimeInformation.ProcessArchitecture);
        Stage(rid, "rulebearing", $"printf '%s\\n' \"$@\" > '{record}'\nexit 7", executable: false);
        using StringWriter error = new();
        Assert.Equal(7, Launcher.Run(["cruise", "a b", "--output-type", "json"], error, directory, rid, null));
        Assert.Equal("cruise\na b\n--output-type\njson\n", File.ReadAllText(record));
        Assert.Empty(error.ToString());
        Assert.NotEqual(UnixFileMode.None, File.GetUnixFileMode(Path.Combine(directory, "runtimes", rid, "native", "rulebearing")) & UnixFileMode.UserExecute);
        Assert.Throws<ArgumentNullException>(() => Launcher.Execute(record, null!));
    }

    /// <summary>A binary that cannot start is exit 2 with the reason.</summary>
    [Fact]
    [UnsupportedOSPlatform("windows")]
    public void ABinaryThatCannotStartIsExitTwo()
    {
        string binary = Path.Combine(directory, "not-a-program");
        File.WriteAllBytes(binary, [0, 1, 2, 3]);
        using StringWriter error = new();
        Assert.Equal(Launcher.CannotRun, Launcher.Run([], error, directory, "linux-x64", binary));
        Assert.Contains("could not run", error.ToString(), StringComparison.Ordinal);
    }

    /// <summary>The entry point runs the binary RULEBEARING_BINARY names.</summary>
    [Fact]
    [UnsupportedOSPlatform("windows")]
    public void TheEntryPointUsesTheEnvironment()
    {
        string binary = Stage("entry", "rulebearing", "exit 5");
        string? saved = Environment.GetEnvironmentVariable(Launcher.BinaryVariable);
        try
        {
            Environment.SetEnvironmentVariable(Launcher.BinaryVariable, binary);
            Assert.Equal(5, Program.Main(["--version"]));
        }
        finally
        {
            Environment.SetEnvironmentVariable(Launcher.BinaryVariable, saved);
        }
    }

    /// <summary>A delivery is cancelled for the launcher and forwarded by name.</summary>
    [Fact]
    public void ASignalIsCancelledAndForwarded()
    {
        Assert.Equal(["INT", "TERM", "HUP"], SignalForwarding.Forwarded.Select(static f => f.Name));
        Assert.Equal([PosixSignal.SIGINT, PosixSignal.SIGTERM, PosixSignal.SIGHUP], SignalForwarding.Forwarded.Select(static f => f.Signal));
        List<string> sent = [];
        PosixSignalContext context = new(PosixSignal.SIGTERM);
        SignalForwarding.Handle(context, "TERM", sent.Add);
        Assert.True(context.Cancel);
        Assert.Equal(["TERM"], sent);
    }

    /// <summary>The handlers register and unregister, and a forwarded SIGTERM stops the child with its own exit code.</summary>
    [Fact]
    [UnsupportedOSPlatform("windows")]
    public void SigtermReachesTheChild()
    {
        if (OperatingSystem.IsWindows())
        {
            return;
        }

        IReadOnlyList<PosixSignalRegistration> registrations = SignalForwarding.Register(static _ => { });
        Assert.Equal(3, registrations.Count);
        foreach (PosixSignalRegistration registration in registrations)
        {
            registration.Dispose();
        }

        string script = Stage("signals", "trap", "trap 'exit 7' TERM\necho ready\nwhile :; do sleep 0.05; done");
        System.Diagnostics.ProcessStartInfo start = new(script) { UseShellExecute = false, RedirectStandardOutput = true };
        using System.Diagnostics.Process child = System.Diagnostics.Process.Start(start)!;
        Assert.Equal("ready", child.StandardOutput.ReadLine());
        Assert.True(SignalForwarding.Send(child.Id, "TERM"));
        Assert.True(child.WaitForExit(10_000));
        Assert.Equal(7, child.ExitCode);
        Assert.False(SignalForwarding.Send(child.Id, "TERM"), "the child is gone");
    }

    /// <summary>
    /// End to end: a SIGTERM to the launcher's process while it runs the binary is forwarded, and
    /// the launcher returns the binary's exit code instead of terminating.
    /// </summary>
    [Fact]
    [UnsupportedOSPlatform("windows")]
    public async Task TheLauncherForwardsSigtermAndReturnsTheChildCode()
    {
        if (OperatingSystem.IsWindows())
        {
            return;
        }

        string ready = Path.Combine(directory, "ready");
        string script = Stage("forward", "rulebearing", $"trap 'exit 9' TERM\ntouch '{ready}'\nwhile :; do sleep 0.05; done");
        Task<int> run = Task.Run(() => Launcher.Execute(script, []));
        for (int i = 0; i < 400 && !File.Exists(ready); i++)
        {
            await Task.Delay(25);
        }

        Assert.True(File.Exists(ready), "the child started");
        Assert.True(SignalForwarding.Send(Environment.ProcessId, "TERM"));
        Assert.Equal(9, await run.WaitAsync(TimeSpan.FromSeconds(10)));
    }

    /// <inheritdoc />
    public void Dispose()
    {
        try
        {
            Directory.Delete(directory, recursive: true);
        }
        catch (IOException)
        {
            // A temporary directory left behind is not a test failure.
        }
    }

    private string Stage(string rid, string name, string body, bool executable = true)
    {
        string folder = Path.Combine(directory, "runtimes", rid, "native");
        Directory.CreateDirectory(folder);
        string path = Path.Combine(folder, name);
        File.WriteAllText(path, $"#!/bin/sh\n{body}\n");
        if (executable && !OperatingSystem.IsWindows())
        {
            File.SetUnixFileMode(path, UnixFileMode.UserRead | UnixFileMode.UserWrite | UnixFileMode.UserExecute);
        }

        return path;
    }
}
