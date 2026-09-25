// The signals that stop a POSIX process, forwarded from the launcher to the binary it runs, so
// `kill`, a container stop (SIGTERM to the launcher alone) or a closed terminal (SIGHUP) stop the
// binary rather than orphan it, and the launcher still exits with the binary's own code.
// Plan: docs/plans/pending/0002-wave-2-dotnet-python-element-rules.md, Step 14 (2H); exit codes
// docs/adr/0008-exit-code-contract.md. Windows keeps Console.CancelKeyPress (src/Launcher.cs): its
// console delivers Ctrl+C to the binary too, and it has no signals to forward.

using System.Diagnostics;
using System.Globalization;
using System.Runtime.InteropServices;
using System.Runtime.Versioning;

namespace Rulebearing.Tool;

/// <summary>Forwards SIGINT, SIGTERM and SIGHUP to the binary.</summary>
internal static class SignalForwarding
{
    /// <summary>The signals forwarded, with the name <c>kill -s</c> takes for each.</summary>
    public static readonly IReadOnlyList<(PosixSignal Signal, string Name)> Forwarded =
        [(PosixSignal.SIGINT, "INT"), (PosixSignal.SIGTERM, "TERM"), (PosixSignal.SIGHUP, "HUP")];

    /// <summary>
    /// Handles one delivery: cancels the launcher's default reaction (terminating), so it keeps
    /// waiting for the binary, and forwards the signal.
    /// </summary>
    /// <param name="context">The delivery.</param>
    /// <param name="name">The signal's name, as <c>kill -s</c> takes it.</param>
    /// <param name="forward">Sends the signal to the binary.</param>
    public static void Handle(PosixSignalContext context, string name, Action<string> forward)
    {
        ArgumentNullException.ThrowIfNull(context);
        ArgumentNullException.ThrowIfNull(forward);
        context.Cancel = true;
        forward(name);
    }

    /// <summary>Registers a handler for each of <see cref="Forwarded"/>; dispose each to unregister.</summary>
    /// <param name="forward">Sends the named signal to the binary.</param>
    /// <returns>The registrations.</returns>
    [UnsupportedOSPlatform("windows")]
    public static IReadOnlyList<PosixSignalRegistration> Register(Action<string> forward) =>
        [.. Forwarded.Select(f => PosixSignalRegistration.Create(f.Signal, context => Handle(context, f.Name, forward)))];

    /// <summary>
    /// Sends a signal to a process with <c>kill -s</c>, which every POSIX system has; .NET has no
    /// managed call that sends one, and the tool carries no native code.
    /// </summary>
    /// <param name="pid">The process.</param>
    /// <param name="name">The signal's name.</param>
    /// <returns>Whether <c>kill</c> reported success.</returns>
    [UnsupportedOSPlatform("windows")]
    public static bool Send(int pid, string name)
    {
        ProcessStartInfo start = new("kill") { UseShellExecute = false, RedirectStandardError = true };
        start.ArgumentList.Add("-s");
        start.ArgumentList.Add(name);
        start.ArgumentList.Add(pid.ToString(CultureInfo.InvariantCulture));
        try
        {
            using Process? kill = Process.Start(start);
            if (kill is null)
            {
                return false;
            }

            kill.StandardError.ReadToEnd();
            kill.WaitForExit();
            return kill.ExitCode == 0;
        }
        catch (System.ComponentModel.Win32Exception)
        {
            return false;
        }
    }
}
