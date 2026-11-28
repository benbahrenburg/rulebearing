# Scale table

Written by testbeds/bench/scale_table.py from the nightly (plan 0003, Step 24).

| Repository | .NET read as | Wall-clock | Peak memory | Modules | Measured in run |
| --- | --- | --- | --- | --- | --- |
| [n8n-io/n8n](https://github.com/n8n-io/n8n/tree/f89687fd8751970cdf8b9cb76761a1258728ca62) | - | 10.13 s | 1995 MB | javascript 1500, python 168, typescript 21244, unknown 4102 | [46141816521](https://github.com/benbahrenburg/rulebearing/actions/runs/46141816521) |
| [grafana/grafana](https://github.com/grafana/grafana/tree/423f85091b0a63a21fb571122a933133da1652cd) | - | 0.85 s | 173 MB | javascript 52, typescript 2077, unknown 351 | [46141816521](https://github.com/benbahrenburg/rulebearing/actions/runs/46141816521) |
| [elastic/kibana](https://github.com/elastic/kibana/tree/b502e688526bca862855361abf0081d84e65c547) | - | 9.4 s | 610 MB | javascript 271, typescript 6825, unknown 390 | [46141816521](https://github.com/benbahrenburg/rulebearing/actions/runs/46141816521) |
| [dotnet/aspnetcore](https://github.com/dotnet/aspnetcore/tree/b4be275a3b4fd83c304e7377403561a73eb4249e) | compiled | 29.23 s | 5612 MB | dotnet 10131, javascript 264, typescript 207, unknown 69 | [46141816521](https://github.com/benbahrenburg/rulebearing/actions/runs/46141816521) |
| [dotnet/aspnetcore](https://github.com/dotnet/aspnetcore/tree/b4be275a3b4fd83c304e7377403561a73eb4249e) | source | 7.72 s | 528 MB | dotnet 10131, javascript 264, typescript 207, unknown 69 | [46141816521](https://github.com/benbahrenburg/rulebearing/actions/runs/46141816521) |
| [jellyfin/jellyfin](https://github.com/jellyfin/jellyfin/tree/fd75964da853765d63101b4889f23a4161e2758b) | compiled | 4.9 s | 972 MB | dotnet 2317 | [46141816521](https://github.com/benbahrenburg/rulebearing/actions/runs/46141816521) |
| [home-assistant/core](https://github.com/home-assistant/core/tree/f50d777440e878554ef6872062341d24c6da0289) | - | 10.25 s | 2212 MB | javascript 3, python 22920, typescript 2, unknown 1 | [46141816521](https://github.com/benbahrenburg/rulebearing/actions/runs/46141816521) |

Stop hook on dotnet/aspnetcore in source mode: p95 1.388 s over 200 edits on 4 CPUs, run [46141816521](https://github.com/benbahrenburg/rulebearing/actions/runs/46141816521).
