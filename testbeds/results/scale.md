# Scale table

Written by testbeds/bench/scale_table.py from the nightly (plan 0003, Step 24).

| Repository | .NET read as | Wall-clock | Peak memory | Modules | Measured in run |
| --- | --- | --- | --- | --- | --- |
| [n8n-io/n8n](https://github.com/n8n-io/n8n/tree/f89687fd8751970cdf8b9cb76761a1258728ca62) | - | 12.06 s | 2034 MB | javascript 1500, python 168, typescript 21244, unknown 4102 | [45017554097](https://github.com/benbahrenburg/rulebearing/actions/runs/45017554097) |
| [grafana/grafana](https://github.com/grafana/grafana/tree/423f85091b0a63a21fb571122a933133da1652cd) | - | 0.8 s | 173 MB | javascript 52, typescript 2077, unknown 351 | [45017554097](https://github.com/benbahrenburg/rulebearing/actions/runs/45017554097) |
| [elastic/kibana](https://github.com/elastic/kibana/tree/b502e688526bca862855361abf0081d84e65c547) | - | 7.2 s | 612 MB | javascript 271, typescript 6825, unknown 390 | [45017554097](https://github.com/benbahrenburg/rulebearing/actions/runs/45017554097) |
| [dotnet/aspnetcore](https://github.com/dotnet/aspnetcore/tree/b4be275a3b4fd83c304e7377403561a73eb4249e) | compiled | 27.12 s | 5611 MB | dotnet 10131, javascript 268, typescript 207, unknown 69 | [45017554097](https://github.com/benbahrenburg/rulebearing/actions/runs/45017554097) |
| [dotnet/aspnetcore](https://github.com/dotnet/aspnetcore/tree/b4be275a3b4fd83c304e7377403561a73eb4249e) | source | 9.05 s | 543 MB | dotnet 10131, javascript 268, typescript 207, unknown 69 | [45017554097](https://github.com/benbahrenburg/rulebearing/actions/runs/45017554097) |
| [jellyfin/jellyfin](https://github.com/jellyfin/jellyfin/tree/fd75964da853765d63101b4889f23a4161e2758b) | compiled | 5.07 s | 973 MB | dotnet 2317 | [45017554097](https://github.com/benbahrenburg/rulebearing/actions/runs/45017554097) |
| [home-assistant/core](https://github.com/home-assistant/core/tree/f50d777440e878554ef6872062341d24c6da0289) | - | 10.2 s | 2213 MB | javascript 3, python 22920, typescript 2, unknown 1 | [45017554097](https://github.com/benbahrenburg/rulebearing/actions/runs/45017554097) |

Stop hook on dotnet/aspnetcore in source mode: p95 1.709 s over 200 edits on 4 CPUs, run [45017554097](https://github.com/benbahrenburg/rulebearing/actions/runs/45017554097).
