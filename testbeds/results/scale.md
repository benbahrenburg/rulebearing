# Scale table

Written by testbeds/bench/scale_table.py from the nightly (plan 0003, Step 24).

| Repository | .NET read as | Wall-clock | Peak memory | Modules | Measured in run |
| --- | --- | --- | --- | --- | --- |
| [n8n-io/n8n](https://github.com/n8n-io/n8n/tree/f89687fd8751970cdf8b9cb76761a1258728ca62) | - | 13.46 s | 1974 MB | javascript 1500, python 168, typescript 21244, unknown 4102 | [42363229969](https://github.com/benbahrenburg/rulebearing/actions/runs/42363229969) |
| [grafana/grafana](https://github.com/grafana/grafana/tree/423f85091b0a63a21fb571122a933133da1652cd) | - | 0.89 s | 180 MB | javascript 52, typescript 2077, unknown 351 | [42363229969](https://github.com/benbahrenburg/rulebearing/actions/runs/42363229969) |
| [elastic/kibana](https://github.com/elastic/kibana/tree/b502e688526bca862855361abf0081d84e65c547) | - | 7.51 s | 610 MB | javascript 271, typescript 6825, unknown 390 | [42363229969](https://github.com/benbahrenburg/rulebearing/actions/runs/42363229969) |
| [dotnet/aspnetcore](https://github.com/dotnet/aspnetcore/tree/b4be275a3b4fd83c304e7377403561a73eb4249e) | compiled | 38.37 s | 5610 MB | dotnet 10131, javascript 268, typescript 207, unknown 69 | [42363229969](https://github.com/benbahrenburg/rulebearing/actions/runs/42363229969) |
| [dotnet/aspnetcore](https://github.com/dotnet/aspnetcore/tree/b4be275a3b4fd83c304e7377403561a73eb4249e) | source | 8.87 s | 522 MB | dotnet 10131, javascript 268, typescript 207, unknown 69 | [42363229969](https://github.com/benbahrenburg/rulebearing/actions/runs/42363229969) |
| [jellyfin/jellyfin](https://github.com/jellyfin/jellyfin/tree/fd75964da853765d63101b4889f23a4161e2758b) | compiled | 4.82 s | 974 MB | dotnet 2317 | [42363229969](https://github.com/benbahrenburg/rulebearing/actions/runs/42363229969) |
| [home-assistant/core](https://github.com/home-assistant/core/tree/f50d777440e878554ef6872062341d24c6da0289) | - | 10.7 s | 2252 MB | javascript 3, python 22920, typescript 2, unknown 1 | [42363229969](https://github.com/benbahrenburg/rulebearing/actions/runs/42363229969) |

Stop hook on dotnet/aspnetcore in source mode: p95 1.864 s over 200 edits on 4 CPUs, run [42363229969](https://github.com/benbahrenburg/rulebearing/actions/runs/42363229969).
