# bgrt — Measured results

Every `bgrt-bench` run on record, how to produce one, and
[which machines are still missing](#data-points-still-wanted).

- Why the harness is built this way — [`DESIGN.md`](DESIGN.md)
- What each class does per OS — [the README](../README.md#qos-classes)

## How to read the tables

There are two workloads, and they show different things.

| Workload | Column that matters | What it shows |
|---|---|---|
| `--workload cpu` (default) | `work/s` | Low-priority work does less. Runs are duration-bounded, so a lower-priority executor finishes fewer work units in the same wall time. |
| `--workload io` | `fg_prot%` | Low-priority work gets out of the way. What a competing foreground reader keeps, as a percentage of its own uncontended throughput. |

`work/s` and the disk columns need no privileges on any platform. `%E`,
frequency, and energy need elevation on some — see
[what is measurable](#what-is-measurable-per-platform).

Anything the OS or the privilege level cannot provide prints `n/a` rather than
failing.

---

# CPU results

## Apple M1 (heterogeneous, `sudo … --mac-power`)

Nothing else running but the harness:

```text
executor              wall_ms       work      work/s     %E  mean_mhz  max_mhz  energy_j
default                  3000      15915        5305   53.8      2410     2812     4.488
utility                  3000      15731        5244   54.7      2380     2761     4.154
background               3000      10130        3376   99.9      2062     2063     1.477
background-threads       3000      10153        3384   99.8      2061     2064     1.474
verdict: background peak frequency ≤ (stayed cool) default
```

`Background` ran 99.9% on the efficiency cores against 53.8% for `Default`,
peaked at 2063 MHz against 2812, and drew about 3 times less CPU power (1.477 J
against 4.488 J over the same 3 s) at about 64% of the throughput. Per unit of
work that is about 1.9 times less energy. This is the main result: work stays on
the efficiency cores, and the clocks and fans stay down.

### The efficiency cores' own clock varies between runs

Three runs on the same M1, `background` row only, against the `default` row from
the same run:

| run | work/s | %E | max_mhz | energy_j | power vs default | energy per work vs default |
|---|---|---|---|---|---|---|
| 1 | 1702 | 99.8 | 1029 | 0.275 | 12.7 times less | 4.1 times less |
| 2 | 1987 | 98.7 | 1284 | 0.470 | 7.2 times less | 2.7 times less |
| 3 (above) | 3376 | 99.9 | 2063 | 1.477 | 3.0 times less | 1.9 times less |

Placement did not move: `Background` sat on the efficiency cores in all three.
What moved is the clock those cores ran at, across most of the M1 E-cluster's
range, and throughput tracked it almost linearly. So the energy advantage is a
range — roughly 3 to 13 times less CPU power, 2 to 4 times less per unit of work
— and the part `bgrt` asks for is the placement. The clock is the OS's call.

`Default`'s own numbers moved too (37.6% to 53.8% on the efficiency cores), so
the ratios above are not a background-only effect. Compare rows within a run,
not across runs.

> The `--mac-power` figures — `%E`, frequency, energy — come from
> `powermetrics`, which reports CPU state for the whole system rather than per
> thread. They are the right lens for fans and battery, but they get noisier when
> other applications are busy. The `work/s` column needs no privileges and is the
> cleanest per-executor signal.

## Linux, AMD Threadripper (homogeneous, 16-core, `sudo`)

```text
executor              wall_ms       work      work/s     %E  mean_mhz  max_mhz  energy_j
default                 15000    4350512      290033    n/a      3687     3692   960.534
utility                 15000    4350058      290003    n/a      3687     3692   982.798
background              15000    4348280      289885    n/a      3687     3692   969.635
background-threads      15000    4349948      289993    n/a      3687     3692   974.817
```

All four executors are identical, which is the expected result here for two
reasons: nothing is competing (one active thread, and `nice` only deprioritizes
when threads compete for a core), and there are no efficiency cores to place work
on. The energy spread under 3% is RAPL noise from reading the whole 16-core
package, not a signal.

## Linux, AMD Threadripper (homogeneous, `sudo … --clamp-frequency`)

Same machine, with the `schedutil` governor confirmed:

```text
executor              wall_ms       work      work/s     %E  mean_mhz  max_mhz  energy_j
default                 10000    2898328      289830    n/a      3685     3692   605.257
utility                 10000    2898107      289809    n/a      3685     3692   614.536
background              10000    1717806      171779    n/a      2188     2200   492.676
background-threads      10000    1723344      172331    n/a      2195     2200   485.833
```

`uclamp` bites where `nice` alone could not: 2188 MHz mean against 3685, about
1.2 times less package energy, at about 59% of the throughput. The drop is
shallower than on the i7 below because a 16-core package has much higher fixed
power, so lowering the per-core clock moves the package total less.

## Linux, Intel i7-2720QM (homogeneous, `--clamp-frequency`)

A 2011 Sandy Bridge, 4 cores and 8 threads, no efficiency cores, `schedutil`
governor. Here the flat homogeneous result turns into a large one:

```text
executor              wall_ms       work      work/s     %E  mean_mhz  max_mhz  energy_j
default                 10000    2499377      249935    n/a      3192     3289   184.103
utility                 10000    2545955      254592    n/a      3245     3289   169.710
background              10000     651358       65135    n/a       840     2990    57.497
background-threads      10000     629097       62906    n/a       802     1295    50.947
```

840 MHz mean against 3192, about 3.8 times lower, and about 3.2 times less CPU
energy over the window (57 J against 184 J) at about a quarter of the throughput.
`Utility` is left unclamped by design and tracks `Default`.

### Frequency clamping keeps the machine cool; it does not save energy per unit of work

Divide energy by work and `Background` here spends slightly *more* per work unit,
about 88 µJ against 74. At low clocks, fixed and leakage power dominate, so on
silicon this old, racing to idle would finish a fixed batch for marginally less
total energy. What clamping buys is lower instantaneous power — a cooler, quieter
machine that is not taking thermal budget from foreground work — rather than a
smaller battery bill for a fixed amount of work.

Efficiency-core placement on macOS is the opposite case, cutting energy about 2
to 4 times per unit of work. Placement and frequency are different levers with
different economics.

---

# Disk results

Low-priority work should give the device up, so every executor runs twice:
alone, and against plain unclassified threads standing in for a foreground
application.
Reads bypass the page cache (`O_DIRECT`, `F_NOCACHE`, `FILE_FLAG_NO_BUFFERING`);
a run that cannot bypass it prints `reads buffered` and warns. No privileges are
needed on any platform, macOS included.

## Apple M1 (APFS on internal NVMe, unprivileged)

```text
$ bgrt-bench --workload io --duration 3 --workers 4 --io-foreground 4
disk: foreground baseline 1349.9 MiB/s, reads direct
executor              solo_mib/s  cont_mib/s    fg_mib/s  fg_prot%    p95_us
default                    993.1       760.1       762.8      56.5       808
utility                    984.6        12.7      1276.4      94.6     22737
background                1097.8         8.8      1299.8      96.3     32892
background-threads        1025.8         9.4      1277.5      94.6     32855
verdict: background left the foreground ≥ (got out of the way) disk throughput than default did
```

- **`Default` splits the device.** Both sides land near 760 MiB/s and the
  foreground keeps 56.5% of its baseline. That is fair sharing, which is correct
  behaviour and also exactly what is not wanted from a backup or an indexer.
- **`Background` gives it up.** About 1100 MiB/s alone drops to 8.8 MiB/s under
  contention, roughly 125 times less, and the foreground keeps 96.3%. Latency
  says the same thing from the other side: 32.9 ms at p95 against 0.81 ms.
  Darwin is pacing those reads rather than queueing them behind the foreground's.
- **It still makes progress.** 8.8 MiB/s is slow, not stopped. That is the
  weighted-fair rule — best-effort, never `IOPRIO_CLASS_IDLE` — showing up as a
  number.
- **`Utility` throttles nearly as hard as `Background`,** at 94.6%. On the CPU
  side `Utility` tracks `Default` closely, so the middle ground the README
  suggests for latency-sensitive low-priority work is not a middle ground on
  disk. That is Apple's mapping of `QOS_CLASS_UTILITY` rather than a `bgrt`
  choice.

Across runs, `default` lands between 56% and 79% and the low-priority classes
between 93% and 100%. The ordering has held every time.

## Saturation matters — read the `default` row first

The same machine at the default `--workers 1 --io-foreground 1`:

```text
executor              solo_mib/s  cont_mib/s    fg_mib/s  fg_prot%    p95_us
default                    420.5       405.4       405.6      97.1       197
background                 288.1         2.3       410.8      98.3     31144
```

`Default` scores 97.1% here, not because it behaved differently but because one
reader at queue depth 1 never saturated the SSD, so there was nothing to contend
for. The `background` row is unchanged, since the macOS throttle has an
unconditional component, but the comparison no longer means anything. `Utility`
has twice come out below `Default` in this configuration (82.7% and 74.9%) while
reading under 3 MiB/s itself — a foreground losing throughput to a competitor
that is barely reading is drift between phases, not contention.

If `default`'s `fg_prot%` is near 100%, the run did not measure contention. The
harness notices this and prints a hint to raise `--workers` and
`--io-foreground`.

---

# Running the harness

Build first, then run the binary. The release profile uses `lto` and
`codegen-units = 1`, so the build is slow and the numbers are worth trusting.

```bash
cargo build --release -p bgrt-bench
```

Any platform, no privileges — the `work/s` comparison and the full disk result:

```bash
./target/release/bgrt-bench --duration 3
./target/release/bgrt-bench --workload io --duration 3 --workers 4 --io-foreground 4
```

macOS. `%E`, frequency, and CPU power come from `powermetrics`, which needs root.
Run the binary under `sudo` rather than `cargo run`: as root, cargo rebuilds and
may not find the toolchain.

```bash
sudo ./target/release/bgrt-bench --duration 3 --mac-power
```

Linux. RAPL energy appears automatically when it is readable; if `energy_j` shows
`n/a`, the counters need root (CVE-2020-8694 restricted them). The two optional
knobs are Linux-only.

```bash
sudo ./target/release/bgrt-bench --duration 3          # RAPL energy
./target/release/bgrt-bench --duration 3 --pin              # hybrid P+E: pin to E-cores
./target/release/bgrt-bench --duration 3 --clamp-frequency  # homogeneous: uclamp cap

# Check the prerequisites before trusting a flat result:
cat /sys/devices/system/cpu/cpufreq/policy0/scaling_governor   # want: schedutil
cat /sys/block/nvme0n1/queue/scheduler                         # want: bfq or mq-deadline
```

Windows. Frequency and placement need no privileges, and there is no energy
counter to elevate for.

```powershell
.\target\release\bgrt-bench.exe --duration 3
```

## What is measurable per platform

| Signal | Linux | Windows | macOS |
|---|---|---|---|
| wall-clock, `work/s`, samples | yes | yes | yes |
| core placement, `%E` | yes, sysfs | yes, `GetSystemCpuSetInformation` | needs `powermetrics` |
| frequency | yes, sysfs | yes, `CallNtPowerInformation` | needs `powermetrics` |
| energy | yes, RAPL (often root) | none exposed | `--mac-power`, needs `sudo` |
| disk throughput, `fg_prot%` | yes, `O_DIRECT` (see below) | yes, `FILE_FLAG_NO_BUFFERING` | yes, `F_NOCACHE` |

The disk workload needs no privileges anywhere, macOS included — no
`powermetrics` involved. On Linux the result still depends on the I/O scheduler:
`bfq` honours priority fully, `mq-deadline` partially, and `none` — a common NVMe
default — not at all. The harness prints the active scheduler, so a flat result
there can be explained rather than guessed at.

The library never needs privileges. Only this measurement tool does.

---

# Data points still wanted

Ordered by what they would settle. A run and its output from one of these
machines is useful, including a flat result, as long as the prerequisites above
were checked first.

| # | Machine or configuration | What it settles | Command |
|--:|---|---|---|
| 1 | Linux on Intel hybrid (Alder, Raptor, Meteor, or Arrow Lake) | The largest gap. E-core detection through the `cpu_atom` PMU, and `sched_setaffinity` against a non-empty core set, have never executed — they are unit-tested only. `--pin` is untested code rather than a measured feature | `bgrt-bench --duration 3 --pin` |
| 2 | Windows, any machine | Nothing on Windows has been measured. CPU throughput under load and the disk behaviour are both reasoned from Microsoft's documentation and from what Chromium ships | `bgrt-bench --workload both --duration 3 --workers 4 --io-foreground 4` |
| 3 | Linux disk, once per I/O scheduler: `bfq`, `mq-deadline`, `none` | Whether the `ioprio_set` mapping bites, and how much the scheduler choice dominates the result. `none` should show nothing, and confirming that is the point | `bgrt-bench --workload io --duration 3 --workers 4 --io-foreground 4` |
| 4 | Linux on arm64 big.LITTLE (Raspberry Pi 5, Snapdragon X, Ampere) | The original `cpu_capacity` detection path — the arm64 interface it was written for — has also never run on real heterogeneous hardware | `bgrt-bench --duration 3 --pin` |
| 5 | Windows on Intel hybrid | The E and P labelling branch in telemetry. CI runners are homogeneous VMs, so the code that marks a core as efficiency-class has only ever been unit-tested. Placement is EcoQoS's job, so this is a telemetry gap rather than a behaviour one | `bgrt-bench --duration 3` |
| 6 | Apple Silicon after the M1 (M2, M3, M4, and a Pro, Max, or Ultra) | Whether efficiency-core confinement and the power ratio hold as the P-to-E ratio changes | `sudo bgrt-bench --duration 3 --mac-power` |
| 8 | The same Mac on battery, on AC, and with Low Power Mode on | What picks the efficiency cluster's clock. Three runs on one M1 span 1029 to 2063 MHz and a 13-to-3-times energy ratio (see [above](#the-efficiency-cores-own-clock-varies-between-runs)); which conditions produce which end is unknown | `sudo bgrt-bench --duration 3 --mac-power` |
| 7 | Any homogeneous Linux machine, with the CPU contended | `nice(19)` does nothing without contention, and every Linux CPU run on record is uncontended, so the CPU half of the class is unproven on Linux | see below |

Row 7 needs two processes. The CPU workload has no foreground-contention mode,
unlike the disk one, so the contention has to come from outside: run a `default`
and a `background` harness against the same cores and compare `work/s` against
each running alone.

```bash
taskset -c 0-3 ./target/release/bgrt-bench --duration 30 --workers 4 --executors default &
taskset -c 0-3 ./target/release/bgrt-bench --duration 30 --workers 4 --executors background &
wait
```

Adding a foreground-contention mode to the CPU workload, in the shape
`--workload io` already uses, would turn that into a single run. It has not been
done.

A result is most useful with the full table, the OS and CPU model, and on Linux
the governor and I/O scheduler from the commands above. A run whose prerequisites
were not checked cannot be told apart from a run where the feature did nothing,
and that is the confusion this project most wants to avoid.
