// The rulebearing binary the tests run: RULEBEARING_BINARY (what CI and adapters/dotnet/test.sh
// set), else the workspace's own build under target/release or target/debug.

namespace Rulebearing.TestAdapter.Fixture;

/// <summary>Finds the binary built from this repository.</summary>
internal static class TestBinary
{
    /// <summary>The binary's full path.</summary>
    /// <exception cref="InvalidOperationException">No binary was built.</exception>
    public static string Path
    {
        get
        {
            string? named = Environment.GetEnvironmentVariable("RULEBEARING_BINARY");
            if (!string.IsNullOrWhiteSpace(named) && File.Exists(named))
            {
                return System.IO.Path.GetFullPath(named);
            }

            string file = OperatingSystem.IsWindows() ? "rulebearing.exe" : "rulebearing";
            for (DirectoryInfo? directory = new(AppContext.BaseDirectory); directory is not null; directory = directory.Parent)
            {
                foreach (string profile in new[] { "release", "debug" })
                {
                    string candidate = System.IO.Path.Combine(directory.FullName, "target", profile, file);
                    if (File.Exists(candidate))
                    {
                        return candidate;
                    }
                }
            }

            throw new InvalidOperationException(
                "No rulebearing binary: set RULEBEARING_BINARY, or build one with `cargo build --release -p rb-cli`.");
        }
    }
}
