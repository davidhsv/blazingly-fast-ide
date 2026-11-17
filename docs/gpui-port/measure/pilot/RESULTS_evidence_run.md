# Measurement results

Runs root: `/private/tmp/openidea-measure-i3NdVc/evidence`.

| run      | status | scenarios degraded | findings |
| -------- | ------ | ------------------ | -------- |
| 1-native | ok     | none               | none     |
| 1-tauri  | ok     | none               | none     |

Counted: runs with status ok; a scenario a run marks degraded (idle not frontmost) is left out of that run.

Each cell: median [min-max] ±spread over the counted repetitions (one value when there is one). Read DESIGN.md's fairness caveats beside every number. Definitions: the docstring of aggregate.py.

The sum of each process's lifetime maximum footprint is no longer given: in the pilot the word read as `ri_lifetime_max_phys_footprint` was below the current footprint 31 times, so the bound it gave was wrong.

## cold_start

| metric                                       | native | Tauri | native / Tauri |
| -------------------------------------------- | ------ | ----- | -------------- |
| duration (s)                                 | 6.0    | 7.0   | 0.86           |
| peak footprint MiB: app                      | 101.5  | 44.1  | 2.30           |
| peak footprint MiB: WebKit                   | 0.0    | 164.7 | 0.00           |
| peak footprint MiB: children                 | 0.0    | 0.0   | -              |
| peak footprint MiB: total                    | 101.5  | 208.8 | 0.49           |
| peak footprint MiB: total without children   | 101.5  | 208.8 | 0.49           |
| steady footprint MiB: app                    | 99.2   | 41.1  | 2.42           |
| steady footprint MiB: WebKit                 | 0.0    | 124.2 | 0.00           |
| steady footprint MiB: children               | 0.0    | 0.0   | -              |
| steady footprint MiB: total                  | 99.2   | 165.2 | 0.60           |
| steady footprint MiB: total without children | 99.2   | 165.2 | 0.60           |
| CPU s: app                                   | 0.3    | 0.5   | 0.75           |
| CPU s: WebKit                                | 0.0    | 0.7   | 0.00           |
| CPU s: children                              | 0.0    | 0.0   | -              |
| CPU s: total                                 | 0.3    | 1.2   | 0.29           |
| CPU s: total without children                | 0.3    | 1.2   | 0.29           |
| CPU % of one core (total)                    | 5.8    | 16.8  | 0.34           |
| wakeups/s (total)                            | 89.2   | 189.8 | 0.47           |
| peak resident MiB: total                     | 67.3   | 336.2 | 0.20           |
| processes (max)                              | 2.0    | 5.0   | 0.40           |
| samples not frontmost                        | 1.0    | 1.0   | 1.00           |

## cold_steady

| metric                                       | native | Tauri | native / Tauri |
| -------------------------------------------- | ------ | ----- | -------------- |
| duration (s)                                 | 31.1   | 30.1  | 1.03           |
| peak footprint MiB: app                      | 99.2   | 43.4  | 2.28           |
| peak footprint MiB: WebKit                   | 0.0    | 128.4 | 0.00           |
| peak footprint MiB: children                 | 0.0    | 0.0   | -              |
| peak footprint MiB: total                    | 99.2   | 171.8 | 0.58           |
| peak footprint MiB: total without children   | 99.2   | 171.8 | 0.58           |
| steady footprint MiB: app                    | 91.8   | 43.3  | 2.12           |
| steady footprint MiB: WebKit                 | 0.0    | 122.6 | 0.00           |
| steady footprint MiB: children               | 0.0    | 0.0   | -              |
| steady footprint MiB: total                  | 91.8   | 166.0 | 0.55           |
| steady footprint MiB: total without children | 91.8   | 166.0 | 0.55           |
| CPU s: app                                   | 0.1    | 0.1   | 0.60           |
| CPU s: WebKit                                | 0.0    | 0.2   | 0.00           |
| CPU s: children                              | 0.0    | 0.0   | -              |
| CPU s: total                                 | 0.1    | 0.2   | 0.21           |
| CPU s: total without children                | 0.1    | 0.2   | 0.21           |
| CPU % of one core (total)                    | 0.2    | 0.8   | 0.20           |
| wakeups/s (total)                            | 87.1   | 118.1 | 0.74           |
| peak resident MiB: total                     | 48.3   | 338.6 | 0.14           |
| processes (max)                              | 2.0    | 5.0   | 0.40           |
| samples not frontmost                        | 0.0    | 0.0   | -              |

## open_project

| metric                                       | native | Tauri | native / Tauri |
| -------------------------------------------- | ------ | ----- | -------------- |
| duration (s)                                 | 43.8   | 45.2  | 0.97           |
| peak footprint MiB: app                      | 250.3  | 224.1 | 1.12           |
| peak footprint MiB: WebKit                   | 0.0    | 240.9 | 0.00           |
| peak footprint MiB: children                 | 0.0    | 7.1   | 0.00           |
| peak footprint MiB: total                    | 250.3  | 437.5 | 0.57           |
| peak footprint MiB: total without children   | 250.3  | 437.5 | 0.57           |
| steady footprint MiB: app                    | 250.3  | 221.5 | 1.13           |
| steady footprint MiB: WebKit                 | 0.0    | 200.9 | 0.00           |
| steady footprint MiB: children               | 0.0    | 0.0   | -              |
| steady footprint MiB: total                  | 250.3  | 422.4 | 0.59           |
| steady footprint MiB: total without children | 250.3  | 422.4 | 0.59           |
| CPU s: app                                   | 14.8   | 17.7  | 0.84           |
| CPU s: WebKit                                | 0.0    | 1.7   | 0.00           |
| CPU s: children                              | 0.0    | 0.0   | 0.00           |
| CPU s: total                                 | 14.8   | 19.5  | 0.76           |
| CPU s: total without children                | 14.8   | 19.5  | 0.76           |
| CPU % of one core (total)                    | 33.8   | 43.1  | 0.79           |
| wakeups/s (total)                            | 82.7   | 144.0 | 0.57           |
| peak resident MiB: total                     | 396.7  | 639.1 | 0.62           |
| processes (max)                              | 2.0    | 9.0   | 0.22           |
| samples not frontmost                        | 1.0    | 1.0   | 1.00           |

## open_files

| metric                                       | native | Tauri | native / Tauri |
| -------------------------------------------- | ------ | ----- | -------------- |
| duration (s)                                 | 165.8  | 197.9 | 0.84           |
| peak footprint MiB: app                      | 299.6  | 252.7 | 1.19           |
| peak footprint MiB: WebKit                   | 0.0    | 525.6 | 0.00           |
| peak footprint MiB: children                 | 0.0    | 13.2  | 0.00           |
| peak footprint MiB: total                    | 299.6  | 777.9 | 0.39           |
| peak footprint MiB: total without children   | 299.6  | 777.9 | 0.39           |
| steady footprint MiB: app                    | 284.7  | 252.1 | 1.13           |
| steady footprint MiB: WebKit                 | 0.0    | 459.5 | 0.00           |
| steady footprint MiB: children               | 0.0    | 0.0   | -              |
| steady footprint MiB: total                  | 284.7  | 711.7 | 0.40           |
| steady footprint MiB: total without children | 284.7  | 711.7 | 0.40           |
| CPU s: app                                   | 7.1    | 8.1   | 0.88           |
| CPU s: WebKit                                | 0.0    | 32.6  | 0.00           |
| CPU s: children                              | 0.0    | 0.0   | 0.00           |
| CPU s: total                                 | 7.1    | 40.7  | 0.18           |
| CPU s: total without children                | 7.1    | 40.7  | 0.18           |
| CPU % of one core (total)                    | 4.3    | 20.6  | 0.21           |
| wakeups/s (total)                            | 118.3  | 378.8 | 0.31           |
| peak resident MiB: total                     | 381.3  | 832.0 | 0.46           |
| processes (max)                              | 2.0    | 6.0   | 0.33           |
| samples not frontmost                        | 0.0    | 0.0   | -              |

## find_in_files

| metric                                       | native | Tauri | native / Tauri |
| -------------------------------------------- | ------ | ----- | -------------- |
| duration (s)                                 | 24.1   | 26.1  | 0.92           |
| peak footprint MiB: app                      | 306.7  | 253.9 | 1.21           |
| peak footprint MiB: WebKit                   | 0.0    | 565.1 | 0.00           |
| peak footprint MiB: children                 | 0.0    | 0.0   | -              |
| peak footprint MiB: total                    | 306.7  | 818.9 | 0.37           |
| peak footprint MiB: total without children   | 306.7  | 818.9 | 0.37           |
| steady footprint MiB: app                    | 302.1  | 252.2 | 1.20           |
| steady footprint MiB: WebKit                 | 0.0    | 515.2 | 0.00           |
| steady footprint MiB: children               | 0.0    | 0.0   | -              |
| steady footprint MiB: total                  | 302.1  | 767.5 | 0.39           |
| steady footprint MiB: total without children | 302.1  | 767.5 | 0.39           |
| CPU s: app                                   | 1.0    | 1.6   | 0.62           |
| CPU s: WebKit                                | 0.0    | 2.4   | 0.00           |
| CPU s: children                              | 0.0    | 0.0   | -              |
| CPU s: total                                 | 1.0    | 4.0   | 0.26           |
| CPU s: total without children                | 1.0    | 4.0   | 0.26           |
| CPU % of one core (total)                    | 4.2    | 15.3  | 0.28           |
| wakeups/s (total)                            | 106.6  | 320.9 | 0.33           |
| peak resident MiB: total                     | 365.4  | 641.5 | 0.57           |
| processes (max)                              | 2.0    | 5.0   | 0.40           |
| samples not frontmost                        | 0.0    | 0.0   | -              |

## git_log

| metric                                       | native | Tauri | native / Tauri |
| -------------------------------------------- | ------ | ----- | -------------- |
| duration (s)                                 | 12.1   | 12.0  | 1.00           |
| peak footprint MiB: app                      | 314.9  | 280.1 | 1.12           |
| peak footprint MiB: WebKit                   | 0.0    | 577.4 | 0.00           |
| peak footprint MiB: children                 | 0.0    | 0.0   | -              |
| peak footprint MiB: total                    | 314.9  | 837.6 | 0.38           |
| peak footprint MiB: total without children   | 314.9  | 837.6 | 0.38           |
| steady footprint MiB: app                    | 311.0  | 278.4 | 1.12           |
| steady footprint MiB: WebKit                 | 0.0    | 540.8 | 0.00           |
| steady footprint MiB: children               | 0.0    | 0.0   | -              |
| steady footprint MiB: total                  | 311.0  | 820.9 | 0.38           |
| steady footprint MiB: total without children | 311.0  | 820.9 | 0.38           |
| CPU s: app                                   | 0.3    | 2.1   | 0.14           |
| CPU s: WebKit                                | 0.0    | 1.5   | 0.00           |
| CPU s: children                              | 0.0    | 0.0   | -              |
| CPU s: total                                 | 0.3    | 3.6   | 0.08           |
| CPU s: total without children                | 0.3    | 3.6   | 0.08           |
| CPU % of one core (total)                    | 2.5    | 29.9  | 0.08           |
| wakeups/s (total)                            | 94.2   | 321.3 | 0.29           |
| peak resident MiB: total                     | 404.3  | 950.6 | 0.43           |
| processes (max)                              | 2.0    | 5.0   | 0.40           |
| samples not frontmost                        | 0.0    | 0.0   | -              |

## deep_mode

| metric                                       | native | Tauri | native / Tauri |
| -------------------------------------------- | ------ | ----- | -------------- |
| duration (s)                                 | 71.3   | 73.3  | 0.97           |
| peak footprint MiB: app                      | 306.1  | 282.1 | 1.09           |
| peak footprint MiB: WebKit                   | 0.0    | 616.6 | 0.00           |
| peak footprint MiB: children                 | 0.0    | 116.0 | 0.00           |
| peak footprint MiB: total                    | 306.1  | 951.7 | 0.32           |
| peak footprint MiB: total without children   | 306.1  | 898.7 | 0.34           |
| steady footprint MiB: app                    | 296.7  | 280.9 | 1.06           |
| steady footprint MiB: WebKit                 | 0.0    | 572.8 | 0.00           |
| steady footprint MiB: children               | 0.0    | 0.0   | -              |
| steady footprint MiB: total                  | 296.7  | 853.8 | 0.35           |
| steady footprint MiB: total without children | 296.7  | 853.8 | 0.35           |
| CPU s: app                                   | 0.4    | 2.0   | 0.19           |
| CPU s: WebKit                                | 0.0    | 3.2   | 0.00           |
| CPU s: children                              | 0.0    | 2.1   | 0.00           |
| CPU s: total                                 | 0.4    | 7.3   | 0.05           |
| CPU s: total without children                | 0.4    | 5.2   | 0.07           |
| CPU % of one core (total)                    | 0.5    | 10.0  | 0.05           |
| wakeups/s (total)                            | 91.6   | 340.2 | 0.27           |
| peak resident MiB: total                     | 410.7  | 936.3 | 0.44           |
| processes (max)                              | 2.0    | 7.0   | 0.29           |
| samples not frontmost                        | 0.0    | 0.0   | -              |

## blaze_again

| metric                                       | native | Tauri | native / Tauri |
| -------------------------------------------- | ------ | ----- | -------------- |
| duration (s)                                 | 71.3   | 71.3  | 1.00           |
| peak footprint MiB: app                      | 333.5  | 280.5 | 1.19           |
| peak footprint MiB: WebKit                   | 0.0    | 583.9 | 0.00           |
| peak footprint MiB: children                 | 0.0    | 0.0   | -              |
| peak footprint MiB: total                    | 333.5  | 864.4 | 0.39           |
| peak footprint MiB: total without children   | 333.5  | 864.4 | 0.39           |
| steady footprint MiB: app                    | 331.2  | 280.0 | 1.18           |
| steady footprint MiB: WebKit                 | 0.0    | 571.4 | 0.00           |
| steady footprint MiB: children               | 0.0    | 0.0   | -              |
| steady footprint MiB: total                  | 331.2  | 851.4 | 0.39           |
| steady footprint MiB: total without children | 331.2  | 851.4 | 0.39           |
| CPU s: app                                   | 0.2    | 1.1   | 0.15           |
| CPU s: WebKit                                | 0.0    | 2.9   | 0.00           |
| CPU s: children                              | 0.0    | 0.0   | -              |
| CPU s: total                                 | 0.2    | 4.1   | 0.04           |
| CPU s: total without children                | 0.2    | 4.1   | 0.04           |
| CPU % of one core (total)                    | 0.2    | 5.7   | 0.04           |
| wakeups/s (total)                            | 89.9   | 332.3 | 0.27           |
| peak resident MiB: total                     | 410.7  | 891.0 | 0.46           |
| processes (max)                              | 2.0    | 5.0   | 0.40           |
| samples not frontmost                        | 0.0    | 0.0   | -              |

## idle

| metric                                       | native | Tauri | native / Tauri |
| -------------------------------------------- | ------ | ----- | -------------- |
| duration (s)                                 | 60.3   | 60.2  | 1.00           |
| peak footprint MiB: app                      | 334.5  | 280.9 | 1.19           |
| peak footprint MiB: WebKit                   | 0.0    | 582.7 | 0.00           |
| peak footprint MiB: children                 | 0.0    | 0.0   | -              |
| peak footprint MiB: total                    | 334.5  | 863.7 | 0.39           |
| peak footprint MiB: total without children   | 334.5  | 863.7 | 0.39           |
| steady footprint MiB: app                    | 334.5  | 280.9 | 1.19           |
| steady footprint MiB: WebKit                 | 0.0    | 573.5 | 0.00           |
| steady footprint MiB: children               | 0.0    | 0.0   | -              |
| steady footprint MiB: total                  | 334.5  | 854.4 | 0.39           |
| steady footprint MiB: total without children | 334.5  | 854.4 | 0.39           |
| CPU s: app                                   | 0.1    | 0.7   | 0.13           |
| CPU s: WebKit                                | 0.0    | 1.8   | 0.00           |
| CPU s: children                              | 0.0    | 0.0   | -              |
| CPU s: total                                 | 0.1    | 2.4   | 0.04           |
| CPU s: total without children                | 0.1    | 2.4   | 0.04           |
| CPU % of one core (total)                    | 0.1    | 4.0   | 0.04           |
| wakeups/s (total)                            | 87.4   | 336.1 | 0.26           |
| peak resident MiB: total                     | 273.8  | 890.4 | 0.31           |
| processes (max)                              | 2.0    | 5.0   | 0.40           |
| samples not frontmost                        | 0.0    | 0.0   | -              |

## Timings and cross-checks

| what                                                                                                         | native | Tauri |
| ------------------------------------------------------------------------------------------------------------ | ------ | ----- |
| cold: /usr/bin/time -l, app process only: maxrss_mib                                                         | 68.9   | 136.1 |
| cold: /usr/bin/time -l, app process only: peak_footprint_mib                                                 | 100.7  | 47.7  |
| cold: /usr/bin/time -l, app process only: real_s                                                             | 40.1   | 40.2  |
| cold: /usr/bin/time -l, app process only: sys_s                                                              | 0.2    | 0.2   |
| cold: /usr/bin/time -l, app process only: user_s                                                             | 0.2    | 0.3   |
| project: /usr/bin/time -l, app process only: maxrss_mib                                                      | 410.2  | 514.6 |
| project: /usr/bin/time -l, app process only: peak_footprint_mib                                              | 333.6  | 391.5 |
| project: /usr/bin/time -l, app process only: real_s                                                          | 468.0  | 504.2 |
| project: /usr/bin/time -l, app process only: sys_s                                                           | 4.5    | 12.0  |
| project: /usr/bin/time -l, app process only: user_s                                                          | 19.6   | 27.6  |
| wait (s) cold: a window is on screen                                                                         | 0.6    | 0.3   |
| wait (s) cold: cold start: CPU < 3% of a core for 5 s                                                        | 6.1    | 7.1   |
| wait (s) project: a window is on screen                                                                      | 0.3    | 0.3   |
| wait (s) project: back in Blaze mode: CPU < 10% of a core for 10 s                                           | 10.2   | 10.1  |
| wait (s) project: deep mode: CPU < 10% of a core for 10 s                                                    | 10.1   | 12.1  |
| wait (s) project: file search File01.java: CPU < 10% of a core for 2 s           | 2.0    | 3.0   |
| wait (s) project: file search File02.java: CPU < 10% of a core for 2 s | 2.0    | 2.0   |
| wait (s) project: file search File03.java: CPU < 10% of a core for 2 s                  | 2.0    | 3.0   |
| wait (s) project: file search File04.java: CPU < 10% of a core for 2 s                            | 2.0    | 2.0   |
| wait (s) project: file search File05.java: CPU < 10% of a core for 2 s                                 | 2.0    | 3.0   |
| wait (s) project: file search File06.java: CPU < 10% of a core for 2 s                            | 2.0    | 3.0   |
| wait (s) project: file search File07.java: CPU < 10% of a core for 2 s                          | 2.0    | 3.0   |
| wait (s) project: file search File08.java: CPU < 10% of a core for 2 s                    | 2.0    | 2.0   |
| wait (s) project: file search File09.java: CPU < 10% of a core for 2 s                       | 2.0    | 2.0   |
| wait (s) project: file search File10.java: CPU < 10% of a core for 2 s                          | 2.0    | 3.0   |
| wait (s) project: find closed: CPU < 10% of a core for 5 s                                                   | 5.1    | 5.1   |
| wait (s) project: find preview: CPU < 10% of a core for 5 s                                                  | 5.1    | 5.0   |
| wait (s) project: find results: CPU < 10% of a core for 5 s                                                  | 6.1    | 7.1   |
| wait (s) project: git log hidden: CPU < 10% of a core for 5 s                                                | 5.1    | 5.1   |
| wait (s) project: git log: CPU < 10% of a core for 5 s                                                       | 5.1    | 5.0   |
| wait (s) project: log: index finished                                                                        | 3.0    | 3.0   |
| wait (s) project: log: opened <project>                                                                      | 0.0    | 0.8   |
| wait (s) project: opened File01.java: CPU < 10% of a core for 2 s                | 2.0    | 4.0   |
| wait (s) project: opened File02.java: CPU < 10% of a core for 2 s      | 2.0    | 3.0   |
| wait (s) project: opened File03.java: CPU < 10% of a core for 2 s                       | 2.0    | 4.1   |
| wait (s) project: opened File04.java: CPU < 10% of a core for 2 s                                 | 2.0    | 4.0   |
| wait (s) project: opened File05.java: CPU < 10% of a core for 2 s                                      | 2.0    | 4.1   |
| wait (s) project: opened File06.java: CPU < 10% of a core for 2 s                                 | 2.0    | 4.0   |
| wait (s) project: opened File07.java: CPU < 10% of a core for 2 s                               | 2.0    | 3.0   |
| wait (s) project: opened File08.java: CPU < 10% of a core for 2 s                         | 2.0    | 4.0   |
| wait (s) project: opened File09.java: CPU < 10% of a core for 2 s                            | 2.0    | 4.1   |
| wait (s) project: opened File10.java: CPU < 10% of a core for 2 s                               | 2.0    | 3.0   |
| wait (s) project: project open: CPU < 3% of a core for 10 s                                                  | 10.1   | 11.1  |
| wait (s) project: scrolled File01.java: CPU < 10% of a core for 3 s              | 3.0    | 6.1   |
| wait (s) project: scrolled File02.java: CPU < 10% of a core for 3 s    | 3.1    | 3.0   |
| wait (s) project: scrolled File03.java: CPU < 10% of a core for 3 s                     | 3.0    | 4.1   |
| wait (s) project: scrolled File04.java: CPU < 10% of a core for 3 s                               | 3.0    | 3.0   |
| wait (s) project: scrolled File05.java: CPU < 10% of a core for 3 s                                    | 3.0    | 4.0   |
| wait (s) project: scrolled File06.java: CPU < 10% of a core for 3 s                               | 3.1    | 5.1   |
| wait (s) project: scrolled File07.java: CPU < 10% of a core for 3 s                             | 3.1    | 5.0   |
| wait (s) project: scrolled File08.java: CPU < 10% of a core for 3 s                       | 3.0    | 4.0   |
| wait (s) project: scrolled File09.java: CPU < 10% of a core for 3 s                          | 3.0    | 5.1   |
| wait (s) project: scrolled File10.java: CPU < 10% of a core for 3 s                             | 3.1    | 4.0   |
| wait (s) project: settings.toml no longer says mode = "full"                                                 | 0.0    | 0.0   |
| wait (s) project: settings.toml says mode = "full"                                                           | 0.0    | 0.0   |
| wait (s) project: title ends with ' – File01.java'                               | 0.0    | 0.0   |
| wait (s) project: title ends with ' – File02.java'                     | 0.0    | 0.0   |
| wait (s) project: title ends with ' – File03.java'                                      | 0.0    | 0.0   |
| wait (s) project: title ends with ' – File04.java'                                                | 0.0    | 0.0   |
| wait (s) project: title ends with ' – File05.java'                                                     | 0.0    | 0.0   |
| wait (s) project: title ends with ' – File06.java'                                                | 0.0    | 0.0   |
| wait (s) project: title ends with ' – File07.java'                                              | 0.0    | 0.0   |
| wait (s) project: title ends with ' – File08.java'                                        | 0.0    | 0.0   |
| wait (s) project: title ends with ' – File09.java'                                           | 0.0    | 0.0   |
| wait (s) project: title ends with ' – File10.java'                                              | 0.0    | 0.3   |

## CPU self-check

- 1-native cold: per-label CPU summed 0.399 s, launch total by the same rule 0.399 s
- 1-native project: per-label CPU summed 23.971 s, launch total by the same rule 23.971 s
- 1-tauri cold: per-label CPU summed 1.429 s, launch total by the same rule 1.429 s
- 1-tauri project: per-label CPU summed 82.169 s, launch total by the same rule 82.169 s
