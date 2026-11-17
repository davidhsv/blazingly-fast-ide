# Measurement results

Runs root: `/private/tmp/openidea-measure-i3NdVc/pilot/attempt2/runs`.

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
| duration (s)                                 | 6.0    | 7.0   | 0.85           |
| peak footprint MiB: app                      | 126.3  | 44.6  | 2.83           |
| peak footprint MiB: WebKit                   | 0.0    | 230.9 | 0.00           |
| peak footprint MiB: children                 | 0.0    | 0.0   | -              |
| peak footprint MiB: total                    | 126.3  | 275.4 | 0.46           |
| peak footprint MiB: total without children   | 126.3  | 275.4 | 0.46           |
| steady footprint MiB: app                    | 124.1  | 42.0  | 2.95           |
| steady footprint MiB: WebKit                 | 0.0    | 143.4 | 0.00           |
| steady footprint MiB: children               | 0.0    | 0.0   | -              |
| steady footprint MiB: total                  | 124.1  | 185.6 | 0.67           |
| steady footprint MiB: total without children | 124.1  | 185.6 | 0.67           |
| CPU s: app                                   | 0.3    | 0.5   | 0.60           |
| CPU s: WebKit                                | 0.0    | 0.8   | 0.00           |
| CPU s: children                              | 0.0    | 0.0   | -              |
| CPU s: total                                 | 0.3    | 1.2   | 0.23           |
| CPU s: total without children                | 0.3    | 1.2   | 0.23           |
| CPU % of one core (total)                    | 4.7    | 17.4  | 0.27           |
| wakeups/s (total)                            | 86.6   | 179.7 | 0.48           |
| peak resident MiB: total                     | 74.6   | 335.0 | 0.22           |
| processes (max)                              | 2.0    | 5.0   | 0.40           |
| samples not frontmost                        | 0.0    | 0.0   | -              |

## cold_steady

| metric                                       | native | Tauri | native / Tauri |
| -------------------------------------------- | ------ | ----- | -------------- |
| duration (s)                                 | 30.2   | 30.2  | 1.00           |
| peak footprint MiB: app                      | 124.1  | 42.7  | 2.90           |
| peak footprint MiB: WebKit                   | 0.0    | 146.1 | 0.00           |
| peak footprint MiB: children                 | 0.0    | 0.0   | -              |
| peak footprint MiB: total                    | 124.1  | 188.8 | 0.66           |
| peak footprint MiB: total without children   | 124.1  | 188.8 | 0.66           |
| steady footprint MiB: app                    | 116.7  | 42.3  | 2.76           |
| steady footprint MiB: WebKit                 | 0.0    | 131.8 | 0.00           |
| steady footprint MiB: children               | 0.0    | 0.0   | -              |
| steady footprint MiB: total                  | 116.7  | 174.1 | 0.67           |
| steady footprint MiB: total without children | 116.7  | 174.1 | 0.67           |
| CPU s: app                                   | 0.1    | 0.2   | 0.41           |
| CPU s: WebKit                                | 0.0    | 0.3   | 0.00           |
| CPU s: children                              | 0.0    | 0.0   | -              |
| CPU s: total                                 | 0.1    | 0.4   | 0.15           |
| CPU s: total without children                | 0.1    | 0.4   | 0.15           |
| CPU % of one core (total)                    | 0.2    | 1.4   | 0.15           |
| wakeups/s (total)                            | 83.0   | 109.5 | 0.76           |
| peak resident MiB: total                     | 74.6   | 313.1 | 0.24           |
| processes (max)                              | 2.0    | 5.0   | 0.40           |
| samples not frontmost                        | 0.0    | 0.0   | -              |

## open_project

| metric                                       | native | Tauri | native / Tauri |
| -------------------------------------------- | ------ | ----- | -------------- |
| duration (s)                                 | 43.2   | 49.2  | 0.88           |
| peak footprint MiB: app                      | 246.7  | 173.8 | 1.42           |
| peak footprint MiB: WebKit                   | 0.0    | 378.7 | 0.00           |
| peak footprint MiB: children                 | 0.0    | 0.0   | -              |
| peak footprint MiB: total                    | 246.7  | 495.0 | 0.50           |
| peak footprint MiB: total without children   | 246.7  | 495.0 | 0.50           |
| steady footprint MiB: app                    | 236.7  | 172.0 | 1.38           |
| steady footprint MiB: WebKit                 | 0.0    | 197.5 | 0.00           |
| steady footprint MiB: children               | 0.0    | 0.0   | -              |
| steady footprint MiB: total                  | 236.7  | 369.6 | 0.64           |
| steady footprint MiB: total without children | 236.7  | 369.6 | 0.64           |
| CPU s: app                                   | 14.8   | 17.4  | 0.85           |
| CPU s: WebKit                                | 0.0    | 1.8   | 0.00           |
| CPU s: children                              | 0.0    | 0.0   | -              |
| CPU s: total                                 | 14.8   | 19.2  | 0.77           |
| CPU s: total without children                | 14.8   | 19.2  | 0.77           |
| CPU % of one core (total)                    | 34.2   | 39.1  | 0.88           |
| wakeups/s (total)                            | 87.1   | 135.0 | 0.65           |
| peak resident MiB: total                     | 387.9  | 773.5 | 0.50           |
| processes (max)                              | 2.0    | 5.0   | 0.40           |
| samples not frontmost                        | 0.0    | 0.0   | -              |

## open_files

| metric                                       | native | Tauri  | native / Tauri |
| -------------------------------------------- | ------ | ------ | -------------- |
| duration (s)                                 | 161.4  | 191.5  | 0.84           |
| peak footprint MiB: app                      | 271.6  | 246.3  | 1.10           |
| peak footprint MiB: WebKit                   | 0.0    | 761.1  | 0.00           |
| peak footprint MiB: children                 | 0.0    | 37.9   | 0.00           |
| peak footprint MiB: total                    | 271.6  | 994.5  | 0.27           |
| peak footprint MiB: total without children   | 271.6  | 994.5  | 0.27           |
| steady footprint MiB: app                    | 258.0  | 245.0  | 1.05           |
| steady footprint MiB: WebKit                 | 0.0    | 503.7  | 0.00           |
| steady footprint MiB: children               | 0.0    | 0.0    | -              |
| steady footprint MiB: total                  | 258.0  | 748.6  | 0.34           |
| steady footprint MiB: total without children | 258.0  | 748.6  | 0.34           |
| CPU s: app                                   | 6.2    | 9.7    | 0.64           |
| CPU s: WebKit                                | 0.0    | 32.8   | 0.00           |
| CPU s: children                              | 0.0    | 0.3    | 0.00           |
| CPU s: total                                 | 6.2    | 42.8   | 0.14           |
| CPU s: total without children                | 6.2    | 42.5   | 0.15           |
| CPU % of one core (total)                    | 3.8    | 22.3   | 0.17           |
| wakeups/s (total)                            | 121.2  | 369.8  | 0.33           |
| peak resident MiB: total                     | 420.7  | 1079.8 | 0.39           |
| processes (max)                              | 2.0    | 6.0    | 0.33           |
| samples not frontmost                        | 0.0    | 0.0    | -              |

## find_in_files

| metric                                       | native | Tauri | native / Tauri |
| -------------------------------------------- | ------ | ----- | -------------- |
| duration (s)                                 | 18.1   | 18.1  | 1.00           |
| peak footprint MiB: app                      | 278.8  | 245.8 | 1.13           |
| peak footprint MiB: WebKit                   | 0.0    | 624.4 | 0.00           |
| peak footprint MiB: children                 | 0.0    | 0.0   | -              |
| peak footprint MiB: total                    | 278.8  | 870.2 | 0.32           |
| peak footprint MiB: total without children   | 278.8  | 870.2 | 0.32           |
| steady footprint MiB: app                    | 276.5  | 245.4 | 1.13           |
| steady footprint MiB: WebKit                 | 0.0    | 572.9 | 0.00           |
| steady footprint MiB: children               | 0.0    | 0.0   | -              |
| steady footprint MiB: total                  | 276.5  | 818.5 | 0.34           |
| steady footprint MiB: total without children | 276.5  | 818.5 | 0.34           |
| CPU s: app                                   | 1.0    | 0.9   | 1.00           |
| CPU s: WebKit                                | 0.0    | 1.3   | 0.00           |
| CPU s: children                              | 0.0    | 0.0   | -              |
| CPU s: total                                 | 1.0    | 2.2   | 0.42           |
| CPU s: total without children                | 1.0    | 2.2   | 0.42           |
| CPU % of one core (total)                    | 5.3    | 12.5  | 0.42           |
| wakeups/s (total)                            | 110.5  | 317.0 | 0.35           |
| peak resident MiB: total                     | 424.4  | 861.9 | 0.49           |
| processes (max)                              | 2.0    | 5.0   | 0.40           |
| samples not frontmost                        | 0.0    | 0.0   | -              |

## git_log

| metric                                       | native | Tauri  | native / Tauri |
| -------------------------------------------- | ------ | ------ | -------------- |
| duration (s)                                 | 18.0   | 17.0   | 1.06           |
| peak footprint MiB: app                      | 310.6  | 251.9  | 1.23           |
| peak footprint MiB: WebKit                   | 0.0    | 589.8  | 0.00           |
| peak footprint MiB: children                 | 0.0    | 0.0    | -              |
| peak footprint MiB: total                    | 310.6  | 841.1  | 0.37           |
| peak footprint MiB: total without children   | 310.6  | 841.1  | 0.37           |
| steady footprint MiB: app                    | 307.3  | 251.4  | 1.22           |
| steady footprint MiB: WebKit                 | 0.0    | 504.6  | 0.00           |
| steady footprint MiB: children               | 0.0    | 0.0    | -              |
| steady footprint MiB: total                  | 307.3  | 756.0  | 0.41           |
| steady footprint MiB: total without children | 307.3  | 756.0  | 0.41           |
| CPU s: app                                   | 1.9    | 2.1    | 0.89           |
| CPU s: WebKit                                | 0.0    | 1.3    | 0.00           |
| CPU s: children                              | 0.0    | 0.0    | -              |
| CPU s: total                                 | 1.9    | 3.4    | 0.55           |
| CPU s: total without children                | 1.9    | 3.4    | 0.55           |
| CPU % of one core (total)                    | 10.4   | 20.0   | 0.52           |
| wakeups/s (total)                            | 106.9  | 302.7  | 0.35           |
| peak resident MiB: total                     | 492.6  | 1190.5 | 0.41           |
| processes (max)                              | 2.0    | 5.0    | 0.40           |
| samples not frontmost                        | 0.0    | 0.0    | -              |

## deep_mode

| metric                                       | native | Tauri  | native / Tauri |
| -------------------------------------------- | ------ | ------ | -------------- |
| duration (s)                                 | 71.2   | 74.2   | 0.96           |
| peak footprint MiB: app                      | 338.5  | 261.6  | 1.29           |
| peak footprint MiB: WebKit                   | 0.0    | 700.6  | 0.00           |
| peak footprint MiB: children                 | 18.2   | 108.1  | 0.17           |
| peak footprint MiB: total                    | 338.5  | 958.5  | 0.35           |
| peak footprint MiB: total without children   | 338.5  | 958.5  | 0.35           |
| steady footprint MiB: app                    | 301.0  | 261.6  | 1.15           |
| steady footprint MiB: WebKit                 | 0.0    | 598.9  | 0.00           |
| steady footprint MiB: children               | 0.0    | 0.0    | -              |
| steady footprint MiB: total                  | 301.0  | 860.4  | 0.35           |
| steady footprint MiB: total without children | 301.0  | 860.4  | 0.35           |
| CPU s: app                                   | 0.3    | 2.4    | 0.14           |
| CPU s: WebKit                                | 0.0    | 4.7    | 0.00           |
| CPU s: children                              | 0.0    | 2.0    | 0.01           |
| CPU s: total                                 | 0.4    | 9.1    | 0.04           |
| CPU s: total without children                | 0.3    | 7.1    | 0.05           |
| CPU % of one core (total)                    | 0.5    | 12.3   | 0.04           |
| wakeups/s (total)                            | 91.1   | 331.3  | 0.27           |
| peak resident MiB: total                     | 524.5  | 1337.8 | 0.39           |
| processes (max)                              | 3.0    | 7.0    | 0.43           |
| samples not frontmost                        | 0.0    | 0.0    | -              |

## blaze_again

| metric                                       | native | Tauri  | native / Tauri |
| -------------------------------------------- | ------ | ------ | -------------- |
| duration (s)                                 | 71.2   | 71.2   | 1.00           |
| peak footprint MiB: app                      | 352.0  | 264.4  | 1.33           |
| peak footprint MiB: WebKit                   | 0.0    | 649.4  | 0.00           |
| peak footprint MiB: children                 | 0.0    | 0.0    | -              |
| peak footprint MiB: total                    | 352.0  | 912.3  | 0.39           |
| peak footprint MiB: total without children   | 352.0  | 912.3  | 0.39           |
| steady footprint MiB: app                    | 344.1  | 262.3  | 1.31           |
| steady footprint MiB: WebKit                 | 0.0    | 605.6  | 0.00           |
| steady footprint MiB: children               | 0.0    | 0.0    | -              |
| steady footprint MiB: total                  | 344.1  | 867.8  | 0.40           |
| steady footprint MiB: total without children | 344.1  | 867.8  | 0.40           |
| CPU s: app                                   | 0.2    | 1.5    | 0.12           |
| CPU s: WebKit                                | 0.0    | 4.0    | 0.00           |
| CPU s: children                              | 0.0    | 0.0    | -              |
| CPU s: total                                 | 0.2    | 5.5    | 0.03           |
| CPU s: total without children                | 0.2    | 5.5    | 0.03           |
| CPU % of one core (total)                    | 0.2    | 7.7    | 0.03           |
| wakeups/s (total)                            | 89.4   | 320.3  | 0.28           |
| peak resident MiB: total                     | 423.6  | 1094.5 | 0.39           |
| processes (max)                              | 2.0    | 5.0    | 0.40           |
| samples not frontmost                        | 0.0    | 0.0    | -              |

## idle

| metric                                       | native | Tauri  | native / Tauri |
| -------------------------------------------- | ------ | ------ | -------------- |
| duration (s)                                 | 601.7  | 599.6  | 1.00           |
| peak footprint MiB: app                      | 371.9  | 272.2  | 1.37           |
| peak footprint MiB: WebKit                   | 0.0    | 719.0  | 0.00           |
| peak footprint MiB: children                 | 0.0    | 0.0    | -              |
| peak footprint MiB: total                    | 371.9  | 986.9  | 0.38           |
| peak footprint MiB: total without children   | 371.9  | 986.9  | 0.38           |
| steady footprint MiB: app                    | 371.9  | 269.6  | 1.38           |
| steady footprint MiB: WebKit                 | 0.0    | 517.4  | 0.00           |
| steady footprint MiB: children               | 0.0    | 0.0    | -              |
| steady footprint MiB: total                  | 371.9  | 787.0  | 0.47           |
| steady footprint MiB: total without children | 371.9  | 787.0  | 0.47           |
| CPU s: app                                   | 1.3    | 9.3    | 0.14           |
| CPU s: WebKit                                | 0.0    | 16.2   | 0.00           |
| CPU s: children                              | 0.0    | 0.0    | -              |
| CPU s: total                                 | 1.3    | 25.6   | 0.05           |
| CPU s: total without children                | 1.3    | 25.6   | 0.05           |
| CPU % of one core (total)                    | 0.2    | 4.3    | 0.05           |
| wakeups/s (total)                            | 85.3   | 197.4  | 0.43           |
| peak resident MiB: total                     | 421.6  | 1094.0 | 0.39           |
| processes (max)                              | 2.0    | 5.0    | 0.40           |
| samples not frontmost                        | 0.0    | 0.0    | -              |

## Timings and cross-checks

| what                                                                   | native | Tauri  |
| ---------------------------------------------------------------------- | ------ | ------ |
| cold: /usr/bin/time -l, app process only: maxrss_mib                   | 73.8   | 136.8  |
| cold: /usr/bin/time -l, app process only: peak_footprint_mib           | 125.4  | 51.0   |
| cold: /usr/bin/time -l, app process only: real_s                       | 39.2   | 40.4   |
| cold: /usr/bin/time -l, app process only: sys_s                        | 0.1    | 0.2    |
| cold: /usr/bin/time -l, app process only: user_s                       | 0.2    | 0.4    |
| project: /usr/bin/time -l, app process only: maxrss_mib                | 496.7  | 622.0  |
| project: /usr/bin/time -l, app process only: peak_footprint_mib        | 371.0  | 328.5  |
| project: /usr/bin/time -l, app process only: real_s                    | 1013.6 | 1051.4 |
| project: /usr/bin/time -l, app process only: sys_s                     | 5.5    | 17.1   |
| project: /usr/bin/time -l, app process only: user_s                    | 20.5   | 32.8   |
| wait (s) cold: a window is on screen                                   | 0.3    | 0.3    |
| wait (s) cold: cold start: CPU < 3% of a core for 5 s                  | 6.1    | 7.1    |
| wait (s) project: a window is on screen                                | 0.3    | 0.3    |
| wait (s) project: back in Blaze mode: CPU < 10% of a core for 10 s     | 10.1   | 10.1   |
| wait (s) project: deep mode: CPU < 10% of a core for 10 s              | 10.1   | 13.1   |
| wait (s) project: file search File01.java: CPU < 10% of a core for 2 s | 2.0    | 3.0    |
| wait (s) project: file search File02.java: CPU < 10% of a core for 2 s | 2.0    | 2.0    |
| wait (s) project: file search File03.java: CPU < 10% of a core for 2 s | 2.0    | 2.0    |
| wait (s) project: file search File04.java: CPU < 10% of a core for 2 s | 2.0    | 3.0    |
| wait (s) project: file search File05.java: CPU < 10% of a core for 2 s | 2.0    | 3.0    |
| wait (s) project: file search File06.java: CPU < 10% of a core for 2 s | 2.0    | 3.0    |
| wait (s) project: file search File07.java: CPU < 10% of a core for 2 s | 2.0    | 2.0    |
| wait (s) project: file search File08.java: CPU < 10% of a core for 2 s | 2.0    | 2.0    |
| wait (s) project: file search File09.java: CPU < 10% of a core for 2 s | 2.0    | 3.0    |
| wait (s) project: file search File10.java: CPU < 10% of a core for 2 s | 2.0    | 3.0    |
| wait (s) project: find closed: CPU < 10% of a core for 5 s             | 5.1    | 5.1    |
| wait (s) project: find preview: CPU < 10% of a core for 5 s            | 5.0    | 5.1    |
| wait (s) project: find results: CPU < 10% of a core for 5 s            | 6.1    | 6.1    |
| wait (s) project: git log hidden: CPU < 10% of a core for 5 s          | 5.1    | 5.0    |
| wait (s) project: git log scrolled: CPU < 10% of a core for 5 s        | 5.1    | 5.1    |
| wait (s) project: git log: CPU < 10% of a core for 5 s                 | 5.1    | 5.1    |
| wait (s) project: log: index finished                                  | 2.0    | 3.1    |
| wait (s) project: log: opened <project>                                | 0.0    | 0.5    |
| wait (s) project: opened File01.java: CPU < 10% of a core for 2 s      | 2.0    | 3.0    |
| wait (s) project: opened File02.java: CPU < 10% of a core for 2 s      | 2.0    | 4.0    |
| wait (s) project: opened File03.java: CPU < 10% of a core for 2 s      | 2.0    | 4.0    |
| wait (s) project: opened File04.java: CPU < 10% of a core for 2 s      | 2.0    | 3.0    |
| wait (s) project: opened File05.java: CPU < 10% of a core for 2 s      | 2.0    | 4.0    |
| wait (s) project: opened File06.java: CPU < 10% of a core for 2 s      | 2.0    | 4.0    |
| wait (s) project: opened File07.java: CPU < 10% of a core for 2 s      | 2.0    | 4.0    |
| wait (s) project: opened File08.java: CPU < 10% of a core for 2 s      | 2.0    | 4.0    |
| wait (s) project: opened File09.java: CPU < 10% of a core for 2 s      | 2.0    | 3.0    |
| wait (s) project: opened File10.java: CPU < 10% of a core for 2 s      | 2.0    | 3.0    |
| wait (s) project: project open: CPU < 3% of a core for 10 s            | 11.1   | 15.2   |
| wait (s) project: scrolled File01.java: CPU < 10% of a core for 3 s    | 3.0    | 3.0    |
| wait (s) project: scrolled File02.java: CPU < 10% of a core for 3 s    | 3.0    | 4.0    |
| wait (s) project: scrolled File03.java: CPU < 10% of a core for 3 s    | 3.0    | 4.1    |
| wait (s) project: scrolled File04.java: CPU < 10% of a core for 3 s    | 3.0    | 3.0    |
| wait (s) project: scrolled File05.java: CPU < 10% of a core for 3 s    | 3.0    | 4.0    |
| wait (s) project: scrolled File06.java: CPU < 10% of a core for 3 s    | 3.0    | 4.0    |
| wait (s) project: scrolled File07.java: CPU < 10% of a core for 3 s    | 3.0    | 5.0    |
| wait (s) project: scrolled File08.java: CPU < 10% of a core for 3 s    | 3.0    | 4.0    |
| wait (s) project: scrolled File09.java: CPU < 10% of a core for 3 s    | 3.0    | 3.0    |
| wait (s) project: scrolled File10.java: CPU < 10% of a core for 3 s    | 3.0    | 4.1    |
| wait (s) project: settings.toml no longer says mode = "full"           | 0.0    | 0.0    |
| wait (s) project: settings.toml says mode = "full"                     | 0.0    | 0.0    |
| wait (s) project: title ends with ' – File01.java'                     | 0.0    | 0.0    |
| wait (s) project: title ends with ' – File02.java'                     | 0.0    | 0.0    |
| wait (s) project: title ends with ' – File03.java'                     | 0.0    | 0.0    |
| wait (s) project: title ends with ' – File04.java'                     | 0.0    | 0.0    |
| wait (s) project: title ends with ' – File05.java'                     | 0.0    | 0.0    |
| wait (s) project: title ends with ' – File06.java'                     | 0.0    | 0.0    |
| wait (s) project: title ends with ' – File07.java'                     | 0.0    | 0.0    |
| wait (s) project: title ends with ' – File08.java'                     | 0.0    | 0.0    |
| wait (s) project: title ends with ' – File09.java'                     | 0.0    | 0.0    |
| wait (s) project: title ends with ' – File10.java'                     | 0.0    | 0.3    |

## CPU self-check

- 1-native cold: per-label CPU summed 0.347 s, launch total by the same rule 0.347 s
- 1-native project: per-label CPU summed 25.747 s, launch total by the same rule 25.747 s
- 1-tauri cold: per-label CPU summed 1.661 s, launch total by the same rule 1.661 s
- 1-tauri project: per-label CPU summed 109.489 s, launch total by the same rule 109.489 s
