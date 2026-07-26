# bgrt — Measured results

Full output from `bgrt-bench` on every machine it has been run on, with the
analysis for each. The [README](../README.md#benchmarking-bgrt-bench) carries the
headline macOS result and how to run the harness; this file is the complete set.

> **These numbers measure CPU only.** The workload is a CPU-bound loop, so
> nothing here exercises the disk half of a `QosClass`. The I/O mapping is
> behaviour-tested per platform but not benchmarked — see
> [`ROADMAP.md`](ROADMAP.md).

The headline signal is **`work/s` (throughput)**: runs are duration-bounded, so a
quieter executor completes *less* work in the same wall time. It is the only
column available unprivileged on every platform.

## Apple M1 (heterogeneous, with `sudo … --mac-power`)

```text
executor              wall_ms       work      work/s     %E  mean_mhz  max_mhz  energy_j
default                  3000      15865        5288   37.6      2124     2751     3.482
utility                  3000      15676        5225   36.3      2095     2719     3.335
background               3000       5106        1702   99.8      1028     1029     0.275
background-threads       3000       5213        1737   98.7      1124     1132     0.491
verdict: background peak frequency ≤ (stayed cool) default
```

`Background` work ran **99.8% on efficiency cores** (vs 37.6% for `Default`),
peaked at **1029 MHz vs 2751 MHz**, and drew **~12× less CPU power** (0.275 J vs
3.482 J over the same 3 s) — at ~32% of the throughput. Per unit of work that's
still ~4× less energy. This is the "stay on the efficiency cores, keep the clocks
and fans down" goal, measured.

> `--mac-power` figures (`%E`, frequency, energy) come from `powermetrics`, which
> reports **system-wide** CPU state, not per-thread — so they reflect total CPU
> activity during the run (the right lens for fans/battery, but noisier if other
> apps are busy). The privilege-free **`work/s`** column is the cleanest
> per-executor signal.

## Linux / AMD Threadripper (homogeneous, 16-core, with `sudo`)

```text
executor              wall_ms       work      work/s     %E  mean_mhz  max_mhz  energy_j
default                 15000    4350512      290033    n/a      3687     3692   960.534
utility                 15000    4350058      290003    n/a      3687     3692   982.798
background              15000    4348280      289885    n/a      3687     3692   969.635
background-threads      15000    4349948      289993    n/a      3687     3692   974.817
verdict: background peak frequency ≤ (stayed cool) default
```

All executors are identical — the expected null result on a homogeneous CPU. Two
reasons `nice(19)` shows nothing here:

1. **No contention:** one active thread, no competing load. `nice` only
   deprioritizes when other threads are competing for the same core.
2. **No E-cores:** Threadripper has no `cpu_capacity` sysfs entries, so
   efficiency-core affinity is a no-op and there's no DVFS difference from nice
   alone.

The `energy_j` variance (<3%) is measurement noise from RAPL reading the entire
16-core package, not per-thread power. Meaningful Linux results need a
heterogeneous (P+E) CPU or a CPU-loaded machine where scheduling priority
actually changes which threads run.

## Linux / AMD Threadripper (homogeneous, with `sudo … --clamp-frequency`)

Same machine, `schedutil` governor confirmed, 10 s run with `sudo` so RAPL energy
is available:

```text
executor              wall_ms       work      work/s     %E  mean_mhz  max_mhz  energy_j
default                 10000    2898328      289830    n/a      3685     3692   605.257
utility                 10000    2898107      289809    n/a      3685     3692   614.536
background              10000    1717806      171779    n/a      2188     2200   492.676
background-threads      10000    1723344      172331    n/a      2195     2200   485.833
verdict: background peak frequency ≤ (stayed cool) default
```

`uclamp` does bite on Threadripper too: `Background` ran at **2188 MHz mean vs
3685 MHz** for `Default` (~1.7× lower clock) and drew **~1.2× less package
energy** (493 J vs 605 J over 10 s), at ~59% of the throughput. The frequency
drop is shallower than on the Sandy Bridge i7 below — Threadripper's 16-core
package has much higher fixed power, so the per-core clock reduction moves the
package needle less. Per-unit-work energy is modestly *worse* for `Background` on
this machine for the same reason as the i7 (see the caveat below).

## Linux / Intel i7-2720QM (homogeneous, with `--clamp-frequency`)

With the `schedutil` governor and `--clamp-frequency`, the homogeneous-CPU null
result flips to dramatic — here on a 2011 Sandy Bridge i7 (4C/8T, no E-cores):

```text
executor              wall_ms       work      work/s     %E  mean_mhz  max_mhz  energy_j
default                 10000    2499377      249935    n/a      3192     3289   184.103
utility                 10000    2545955      254592    n/a      3245     3289   169.710
background              10000     651358       65135    n/a       840     2990    57.497
background-threads      10000     629097       62906    n/a       802     1295    50.947
verdict: background peak frequency ≤ (stayed cool) default
```

`Background` ran at a **840 MHz mean clock vs 3192 MHz** for `Default` (~3.8×
lower) and drew **~3.2× less CPU energy over the 10 s window** (57 J vs 184 J),
at ~26% of the throughput. This is `uclamp` doing exactly what `nice` alone could
not on a homogeneous CPU. `Utility` is left unclamped by design and tracks
`Default`.

### Honest caveat — a stay-cool lever, not a per-unit-work efficiency win

Dividing energy by work, `Background` here actually spends *slightly more* per
work-unit (~88 vs ~74 µJ): at low clocks, fixed and leakage power dominate, so on
this old silicon "race to idle" would finish a fixed batch for marginally less
total energy. The payoff is lower *instantaneous* power, a cooler and quieter
machine, and not stealing thermal/power budget from foreground work — not a
smaller battery bill for a fixed amount of work.

Contrast the macOS result above, where efficiency-core *placement* cuts energy
~4× **per unit of work**: placement and frequency are distinct levers with
distinct economics.
