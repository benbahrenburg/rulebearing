# Scale table

Written by testbeds/bench/scale_table.py from the nightly (plan 0003, Step 24).

| Repository | .NET read as | Wall-clock | Peak memory | Modules | Measured in run |
| --- | --- | --- | --- | --- | --- |
| [n8n-io/n8n](https://github.com/n8n-io/n8n/tree/f89687fd8751970cdf8b9cb76761a1258728ca62) | - | 11.2 s | 2028 MB | javascript 1500, python 168, typescript 21244, unknown 4102 | [38999037867](https://github.com/benbahrenburg/rulebearing/actions/runs/38999037867) |
| [grafana/grafana](https://github.com/grafana/grafana/tree/423f85091b0a63a21fb571122a933133da1652cd) | - | 0.74 s | 182 MB | javascript 52, typescript 2077, unknown 351 | [38999037867](https://github.com/benbahrenburg/rulebearing/actions/runs/38999037867) |
| [elastic/kibana](https://github.com/elastic/kibana/tree/b502e688526bca862855361abf0081d84e65c547) | - | 6.65 s | 613 MB | javascript 271, typescript 6825, unknown 390 | [38999037867](https://github.com/benbahrenburg/rulebearing/actions/runs/38999037867) |
| [dotnet/aspnetcore](https://github.com/dotnet/aspnetcore/tree/b4be275a3b4fd83c304e7377403561a73eb4249e) | compiled | 29.27 s | 5614 MB | dotnet 10131, javascript 268, typescript 207, unknown 69 | [38999037867](https://github.com/benbahrenburg/rulebearing/actions/runs/38999037867) |
| [dotnet/aspnetcore](https://github.com/dotnet/aspnetcore/tree/b4be275a3b4fd83c304e7377403561a73eb4249e) | source | 8.49 s | 539 MB | dotnet 10131, javascript 268, typescript 207, unknown 69 | [38999037867](https://github.com/benbahrenburg/rulebearing/actions/runs/38999037867) |
| [jellyfin/jellyfin](https://github.com/jellyfin/jellyfin/tree/fd75964da853765d63101b4889f23a4161e2758b) | compiled | 4.8 s | 975 MB | dotnet 2317 | [38999037867](https://github.com/benbahrenburg/rulebearing/actions/runs/38999037867) |
| [home-assistant/core](https://github.com/home-assistant/core/tree/f50d777440e878554ef6872062341d24c6da0289) | - | 9.18 s | 2258 MB | javascript 3, python 22920, typescript 2, unknown 1 | [38999037867](https://github.com/benbahrenburg/rulebearing/actions/runs/38999037867) |

Stop hook on dotnet/aspnetcore in source mode: p95 1.579 s over 200 edits on 4 CPUs, run [38999037867](https://github.com/benbahrenburg/rulebearing/actions/runs/38999037867).
