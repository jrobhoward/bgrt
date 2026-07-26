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

```text
executor              wall_ms       work      work/s     %E  mean_mhz  max_mhz  energy_j
default                  3000      15865        5288   37.6      2124     2751     3.482
utility                  3000      15676        5225   36.3      2095     2719     3.335
background               3000       5106        1702   99.8      1028     1029     0.275
background-threads       3000       5213        1737   98.7      1124     1132     0.491
verdict: background peak frequency ≤ (stayed cool) default
```

`Background` ran 99.8% on the efficiency cores against 37.6% for `Default`,
peaked at 1029 MHz against 2751, and drew about 12 times less CPU power (0.275 J
against 3.482 J over the same 3 s) at about a third of the throughput. Per unit
of work that is still about 4 times less energy. This is the main result: work
stays on the efficiency cores, and the clocks and fans stay down.

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

Efficiency-core placement on macOS is the opposite case, cutting energy about 4
times per unit of work. Placement and frequency are different levers with
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
disk: foreground baseline 1301.2 MiB/s, reads direct
executor              solo_mib/s  cont_mib/s    fg_mib/s  fg_prot%    p95_us
default                    988.5       996.6       998.7      76.8       394
utility                   1058.4        12.7      1267.9      97.4     22770
background                1003.0         7.9      1295.3      99.6     32870
background-threads        1021.6         8.0      1294.0      99.5     32872
verdict: background left the foreground ≥ (got out of the way) disk throughput than default did
```

- **`Default` splits the device.** Both sides land near 1000 MiB/s and the
  foreground keeps 76.8% of its baseline. That is fair sharing, which is correct
  behaviour and also exactly what is not wanted from a backup or an indexer.
- **`Background` gives it up.** About 1000 MiB/s alone drops to 7.9 MiB/s under
  contention, roughly 125 times less, and the foreground keeps 99.6%. Latency
  says the same thing from the other side: 32.9 ms at p95 against 0.39 ms.
  Darwin is pacing those reads rather than queueing them behind the foreground's.
- **It still makes progress.** 7.9 MiB/s is slow, not stopped. That is the
  weighted-fair rule — best-effort, never `IOPRIO_CLASS_IDLE` — showing up as a
  number.
- **`Utility` throttles nearly as hard as `Background`,** at 97.4%. On the CPU
  side `Utility` tracks `Default` closely, so the middle ground the README
  suggests for latency-sensitive low-priority work is not a middle ground on
  disk. That is Apple's mapping of `QOS_CLASS_UTILITY` rather than a `bgrt`
  choice.

Across runs, `default` lands between 62% and 79% and the low-priority classes
between 95% and 100%. The ordering has held every time.

## Saturation matters — read the `default` row first

The same machine at the default `--workers 1 --io-foreground 1`:

```text
executor              solo_mib/s  cont_mib/s    fg_mib/s  fg_prot%    p95_us
default                    408.0       396.4       396.3      98.4       196
background                 268.2         2.0       401.3      99.6     31108
```

`Default` scores 98.4% here, not because it behaved differently but because one
reader at queue depth 1 never saturated the SSD, so there was nothing to contend
for. The `background` row is unchanged, since the macOS throttle has an
unconditional component, but the comparison no longer means anything.

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
| 6 | Apple Silicon after the M1 (M2, M3, M4, and a Pro, Max, or Ultra) | Whether efficiency-core confinement and the 12-times power result hold as the P-to-E ratio changes | `sudo bgrt-bench --duration 3 --mac-power` |
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
