# Scale table

Written by testbeds/bench/scale_table.py from the nightly (plan 0003, Step 24).

| Repository | .NET read as | Wall-clock | Peak memory | Modules | Measured in run |
| --- | --- | --- | --- | --- | --- |
| n8n-io/n8n | - | - | - | - | not measured yet |
| grafana/grafana | - | - | - | - | not measured yet |
| elastic/kibana | - | - | - | - | not measured yet |
| [dotnet/aspnetcore](https://github.com/dotnet/aspnetcore/tree/b4be275a3b4fd83c304e7377403561a73eb4249e) | compiled | 799.76 s | 14954 MB | dotnet 11487, javascript 268, typescript 207, unknown 69 | [37862902868](https://github.com/benbahrenburg/rulebearing/actions/runs/37862902868) |
| dotnet/aspnetcore | source | - | - | - | not measured yet |
| [jellyfin/jellyfin](https://github.com/jellyfin/jellyfin/tree/fd75964da853765d63101b4889f23a4161e2758b) | compiled | 12.81 s | 5120 MB | dotnet 2311 | [37880270909](https://github.com/benbahrenburg/rulebearing/actions/runs/37880270909) |
| [home-assistant/core](https://github.com/home-assistant/core/tree/f50d777440e878554ef6872062341d24c6da0289) | - | 11.72 s | 3973 MB | javascript 3, python 22920, typescript 2, unknown 1 | [37880270909](https://github.com/benbahrenburg/rulebearing/actions/runs/37880270909) |

Stop hook on dotnet/aspnetcore in source mode: p95 1.896 s over 200 edits on 4 CPUs, run [37880270909](https://github.com/benbahrenburg/rulebearing/actions/runs/37880270909).
