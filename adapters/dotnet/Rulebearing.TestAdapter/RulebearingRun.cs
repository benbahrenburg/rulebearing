// Runs the binary once per set of options and turns its JSON result into one RuleResult per rule.
//
// Plan: docs/plans/pending/0002-wave-2-dotnet-python-element-rules.md, Step 14 (2H).
// Exit codes: docs/adr/0008-exit-code-contract.md. `-T json` exits 0 whatever it found, 2 when the
// run cannot be trusted (a vacuous rule under strict liveness, docs/adr/0007) and 3 for an
// invalid configuration. Exit 2 whose cause the result carries (a vacuous rule, an expired rule or
// known violation, a ratchet over its ceiling or without a budget) yields results that fail with
// that reason; an exit 2 whose every rule passed, and exit 3, is a RulebearingException, never a pass.

using System.Collections.Concurrent;
using System.Diagnostics;
using System.Text;
using System.Text.Json;

namespace Rulebearing.TestAdapter;

/// <summary>Runs Rulebearing and reads its result into one <see cref="RuleResult"/> per rule.</summary>
public static class RulebearingRun
{
    private static readonly ConcurrentDictionary<string, Lazy<IReadOnlyList<RuleResult>>> Runs = new(StringComparer.Ordinal);

    /// <summary>
    /// The rule results for <paramref name="options"/>. The binary runs once per distinct set of
    /// options in a test run: test frameworks ask for data at discovery and again at execution.
    /// </summary>
    /// <param name="options">What to run.</param>
    /// <returns>One result per rule, in the order the <c>junit</c> reporter lists them.</returns>
    /// <exception cref="RulebearingException">The binary could not be run or its result cannot be trusted.</exception>
    public static IReadOnlyList<RuleResult> Load(RulebearingOptions options)
    {
        ArgumentNullException.ThrowIfNull(options);
        Invocation invocation = Invocation.Plan(options);
        return Runs.GetOrAdd(invocation.Key, _ => new Lazy<IReadOnlyList<RuleResult>>(invocation.Execute)).Value;
    }

    /// <summary>The rule results of a result that <c>rulebearing cruise -T json</c> printed.</summary>
    /// <param name="json">The JSON result.</param>
    /// <returns>One result per rule, in the order the <c>junit</c> reporter lists them.</returns>
    /// <exception cref="RulebearingException">The text is not a Rulebearing result.</exception>
    public static IReadOnlyList<RuleResult> Parse(string json)
    {
        ArgumentNullException.ThrowIfNull(json);
        try
        {
            using JsonDocument document = JsonDocument.Parse(json);
            if (document.RootElement.ValueKind != JsonValueKind.Object
                || !document.RootElement.TryGetProperty("summary", out _))
            {
                throw new RulebearingException(
                    "The result has no `summary`: it is not the output of `rulebearing cruise -T json`. Fix: save the result with `-T json`, not another output type.");
            }

            return Catalog.Cases(document.RootElement);
        }
        catch (JsonException e)
        {
            throw new RulebearingException(
                $"The result is not JSON ({e.Message}). Fix: run `rulebearing cruise -T json`, which prints the result the adapter reads.",
                e);
        }
    }

    /// <summary>One planned run of the binary: resolved paths and the command line.</summary>
    internal sealed record Invocation(
        string? ResultFile,
        string Binary,
        string WorkingDirectory,
        IReadOnlyList<string> Arguments,
        TimeSpan Timeout)
    {
        /// <summary>What identifies this run in the cache.</summary>
        public string Key => ResultFile is not null
            ? $"result\0{ResultFile}"
            : string.Join('\0', [Binary, WorkingDirectory, .. Arguments]);

        /// <summary>Resolves every path of <paramref name="options"/> and builds the command line.</summary>
        public static Invocation Plan(RulebearingOptions options)
        {
            if (options.Result is { } saved)
            {
                return new Invocation(PathSearch.File(saved, "result"), string.Empty, string.Empty, [], options.Timeout);
            }

            string? config = options.Config is { } c ? PathSearch.File(c, "configuration") : null;
            string directory = options.WorkingDirectory is { } w
                ? PathSearch.Directory(w)
                : config is not null ? Path.GetDirectoryName(config) ?? Environment.CurrentDirectory : Environment.CurrentDirectory;
            List<string> arguments = ["cruise"];
            if (config is not null)
            {
                arguments.Add("--config");
                arguments.Add(Path.GetRelativePath(directory, config));
            }

            // --output-to -: a configuration's options.outputTo must neither hide the JSON nor have
            // a file of the user's overwritten with it.
            arguments.AddRange(["--output-type", "json", "--output-to", "-", "--no-progress"]);
            if (options.Graph is { } graph)
            {
                arguments.Add("--graph");
                arguments.Add(PathSearch.File(graph, "graph"));
            }

            arguments.AddRange(options.Arguments);
            return new Invocation(null, PathSearch.Binary(options.Binary), directory, arguments, options.Timeout);
        }

        /// <summary>Reads the saved result, or runs the binary and reads what it printed.</summary>
        public IReadOnlyList<RuleResult> Execute()
        {
            if (ResultFile is not null)
            {
                return Parse(File.ReadAllText(ResultFile));
            }

            (int exit, string stdout, string stderr) = Run();
            string command = $"`{Binary} {string.Join(' ', Arguments)}` in {WorkingDirectory}";
            if (exit == 3)
            {
                throw new RulebearingException(
                    $"The configuration is invalid (exit 3) for {command}:\n{stderr.Trim()}\nFix: correct the configuration; `rulebearing cruise` prints the same error.");
            }

            string untrusted = $"The run cannot be trusted (exit 2) for {command}:\n{stderr.Trim()}\nFix: the message above names the cause (zero modules, an unsupported file, an assembly without a portable PDB).";
            if (exit == 2 && stdout.Trim().Length == 0)
            {
                throw new RulebearingException(untrusted);
            }

            if ((exit != 0 && exit != 2) || stdout.Trim().Length == 0)
            {
                throw new RulebearingException(
                    $"Rulebearing exited {exit} without a result for {command}:\n{stderr.Trim()}");
            }

            IReadOnlyList<RuleResult> results = Parse(stdout);
            if (exit == 2 && results.All(static r => r.Outcome == RuleOutcome.Passed))
            {
                throw new RulebearingException(untrusted);
            }

            return results;
        }

        private (int Exit, string Stdout, string Stderr) Run()
        {
            ProcessStartInfo start = new(Binary)
            {
                WorkingDirectory = WorkingDirectory,
                RedirectStandardOutput = true,
                RedirectStandardError = true,
                RedirectStandardInput = true,
                UseShellExecute = false,
                StandardOutputEncoding = Encoding.UTF8,
                StandardErrorEncoding = Encoding.UTF8,
            };
            foreach (string argument in Arguments)
            {
                start.ArgumentList.Add(argument);
            }

            using Process process = new() { StartInfo = start };
            try
            {
                process.Start();
            }
            catch (System.ComponentModel.Win32Exception e)
            {
                throw new RulebearingException(
                    $"Could not start {Binary}: {e.Message}. Fix: set {RulebearingOptions.BinaryVariable} to the rulebearing binary, or install it with `dotnet tool install -g Rulebearing`.",
                    e);
            }

            process.StandardInput.Close();
            Task<string> stdout = process.StandardOutput.ReadToEndAsync();
            Task<string> stderr = process.StandardError.ReadToEndAsync();
            if (!process.WaitForExit(Timeout))
            {
                process.Kill(entireProcessTree: true);
                throw new RulebearingException(
                    $"Rulebearing did not finish within {Timeout} in {WorkingDirectory}. Fix: raise the timeout, or narrow the run with arguments or a graph.");
            }

            process.WaitForExit();
            return (process.ExitCode, stdout.GetAwaiter().GetResult(), stderr.GetAwaiter().GetResult());
        }
    }
}
