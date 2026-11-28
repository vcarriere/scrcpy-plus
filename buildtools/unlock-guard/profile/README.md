# Test-only diagnostic timing profile

Build from the project root with the existing pinned local tools:

```powershell
./buildtools/unlock-guard/Build-Profile.ps1
./buildtools/unlock-guard/Build-Profile.ps1 -Trials 3
```

The default artifact is `artifacts/unlock-guard-profile-5.jar`, accompanied by its
own JSON receipt. The three-trial build uses a separate `-3` filename. Neither
build changes the normal embedded helper, normal protocol, or normal receipt;
the build script verifies their hashes remain unchanged.

The Android entry point is `com.scrcpyplus.unlockguard.MainProfile`, with no
arguments or stdin commands. It reflects only the existing helper's own private
Android wrapper, running one complete normal filtered snapshot, five sequential
trials of its seven bounded service dumps, and a second complete normal filtered
snapshot. Every dump receives a fresh filter instance. No queries run in parallel,
and no input, power, credential, or authentication operation is performed.

Success is emitted atomically to raw file descriptor 1:

```text
PROFILE_BEFORE
<normal complete @@identity ... @@end snapshot>
PROFILE_TIMINGS
policy=<microseconds>
displays=<microseconds>
focus_display=<microseconds>
input_method=<microseconds>
lock_settings=<microseconds>
power=<microseconds>
SurfaceFlinger=<microseconds>
<the same seven labels repeated for the remaining sequential trials>
PROFILE_AFTER
<normal complete @@identity ... @@end snapshot>
```

Each duration measures the existing bounded dump method, including its pipe,
Binder invocation, filtering, and worker completion. Timed dump values are
discarded. The wrapping snapshots retain the normal identity checks and allow
the native test caller to verify both responses against the selected target.
The caller must reject absent, incomplete, or invalid framing/snapshots and keep
its own bounded timeout and process cleanup.

The standalone entry point has its own 2,500 ms daemon watchdog because normal
`Main.run` is bypassed. Any failure or timeout emits only `PROFILE_UNAVAILABLE`;
exceptions, unfiltered dumps, and framework System-stream diagnostics are
discarded. An unsupported-host fixed-failure probe runs during the build, along
with the normal helper's synthetic checks. Real-device execution belongs to the
explicitly scoped native read-only benchmark; this build script never runs ADB.

## Separate Power text/protobuf comparison

```powershell
./buildtools/unlock-guard/Build-Profile.ps1 -Mode PowerProto
./buildtools/unlock-guard/Build-Profile.ps1 -Mode PowerProto -Trials 3
```

This creates `artifacts/unlock-guard-power-proto-profile-5.jar` and its own JSON
receipt. The entry point is `com.scrcpyplus.unlockguard.MainPowerProfile`, with
no arguments or stdin commands. It keeps the same wrapping `PROFILE_BEFORE` and
`PROFILE_AFTER` normal snapshots, but each timing trial contains exactly:

```text
power_text=<microseconds>
power_text_wakefulness=<0..3>
power_proto=<microseconds>
power_proto_wakefulness=<-1..3>
```

The text and protobuf query order alternates between trials; output label order
remains fixed. Both durations include their full bounded Binder dump, pipe drain,
state decoding, and worker completion. Text accepts exactly one normal filtered
main symbolic wakefulness field. Protobuf is streamed directly from the Power
service with `--proto`; raw protobuf is never printed or written to disk.

Official Android 15–17 sources agree that `PowerManagerServiceDumpProto` root
field 3 is the wire-type-0 Wakefulness enum: ASLEEP=0, AWAKE=1, DREAMING=2,
DOZING=3. Both text and protobuf read `getGlobalWakefulnessLocked()` under the
service lock. `ProtoOutputStream` omits a single enum's default zero, so `-1`
means the root field was absent after a complete valid wire-framing parse. It
does not infer asleep, awake, or any other state. Running this test never promotes
an optimization. After the measured prototype, the production helper now uses
the shared parser and confirms an absent field with an authoritative fresh text
query inside the same helper, at the original late Power-query position.

The decoder validates all root wire framing through EOF, including unknown
fields, 64-bit varint bounds/canonical encodings, lengths, fixed-width fields,
group matching, and bounded group depth. It rejects duplicate root state,
incorrect root wire type, enum values outside 0–3, malformed/truncated data, and
input exceeding 4 MiB. Length-delimited unknown fields remain opaque because
protobuf wire framing does not distinguish strings, bytes, and nested messages;
nested-looking bytes never become root wakefulness. Only the root enum is
extracted. The one production `PowerProto` parser and `PowerDump` pipe reader are
shared with this profiler; neither is duplicated in profile source. All helper,
strict parser, and Power fallback/freshness tests run for either profile build
mode. The same independent 2,500 ms watchdog and fixed
`PROFILE_UNAVAILABLE` failure output apply. Normal source, protocol, embedded
JAR, receipt, and late power-query order stay unchanged by profile builds.

Primary Android 17 source revision
`94b4c163b7dfe5ce3607f7bb8456f9573f7de57d`:

- [PowerManagerService dump schema](https://android.googlesource.com/platform/frameworks/base/+/94b4c163b7dfe5ce3607f7bb8456f9573f7de57d/core/proto/android/server/powermanagerservice.proto)
- [Wakefulness enum](https://android.googlesource.com/platform/frameworks/base/+/94b4c163b7dfe5ce3607f7bb8456f9573f7de57d/core/proto/android/os/powermanager.proto)
- [Shared text/protobuf state source](https://android.googlesource.com/platform/frameworks/base/+/94b4c163b7dfe5ce3607f7bb8456f9573f7de57d/services/core/java/com/android/server/power/PowerManagerService.java)
- [Default single-enum omission](https://android.googlesource.com/platform/frameworks/base/+/94b4c163b7dfe5ce3607f7bb8456f9573f7de57d/core/java/android/util/proto/ProtoOutputStream.java)

The corresponding Android 15 revision
`cc8bb19595c661f5cf42f330457000e643c67d1b` and Android 16 revision
`99b01a65cc4c104933788b3143285ab6bae65827` were checked for the same field,
enum, state source, and default-omission behavior.

## Separate normal/critical IME comparison

```powershell
./buildtools/unlock-guard/Build-Profile.ps1 -Mode ImeCritical
./buildtools/unlock-guard/Build-Profile.ps1 -Mode ImeCritical -Trials 3
```

This creates `artifacts/unlock-guard-ime-critical-profile-5.jar` and its own JSON
receipt. The entry point is `com.scrcpyplus.unlockguard.MainImeCriticalProfile`,
with no arguments or stdin commands. The profiler uses the existing private
Android dump wrapper and a new `ImeFilter` for every sample. It performs one
complete normal snapshot, five alternating normal/critical IME dump pairs, and a
second complete normal snapshot. It does not change the production helper's IME
arguments or promote a result into normal operation.

The comparison has the same `PROFILE_BEFORE`, `PROFILE_TIMINGS`, and
`PROFILE_AFTER` framing as the other profilers. Each pair emits exactly:

```text
ime_normal=<microseconds>
ime_critical=<microseconds>
ime_equal=<0|1>
```

Normal uses no dump arguments; critical uses `--dump-priority CRITICAL`. Each
duration includes Binder invocation, bounded pipe draining, filtering, and worker
completion. `ime_equal=1` means the complete filtered strings (and therefore their
UTF-8 encodings) are byte-identical for that pair; unequal output is reported as
zero, never hidden or retried. Raw IME output, history, editor contents, and the
individual filtered IME strings are not emitted. The native caller must validate
both complete snapshots against the selected target and reject a comparison with
any zero equality result or invalid/missing framing before drawing conclusions.

Android 15/16/17 `dumpCritical` preserves the server's current metadata but exits
before remote IME/client `TransferPipe.dumpAsync` work. Android 16/17 current
user/repository/editor fields precede that exit; Android 15's older diagnostic
layout still needs its own adapter. References:

- [Android 17 IME critical dispatcher and dump](https://android.googlesource.com/platform/frameworks/base/+/94b4c163b7dfe5ce3607f7bb8456f9573f7de57d/services/core/java/com/android/server/inputmethod/InputMethodManagerService.java#6413)
- [Android 16 equivalent](https://android.googlesource.com/platform/frameworks/base/+/99b01a65cc4c104933788b3143285ab6bae65827/services/core/java/com/android/server/inputmethod/InputMethodManagerService.java#6014)
- [Android 15 equivalent](https://android.googlesource.com/platform/frameworks/base/+/cc8bb19595c661f5cf42f330457000e643c67d1b/services/core/java/com/android/server/inputmethod/InputMethodManagerService.java#6002)
- [PriorityDump argument handling](https://android.googlesource.com/platform/frameworks/base/+/94b4c163b7dfe5ce3607f7bb8456f9573f7de57d/services/core/java/com/android/server/utils/PriorityDump.java#126)

## Separate twelve-snapshot rehearsal

```powershell
./buildtools/unlock-guard/Build-Profile.ps1 -Mode Rehearsal
```

This creates `artifacts/unlock-guard-rehearsal-profile-12.jar` and its own receipt.
The entry point is `com.scrcpyplus.unlockguard.MainRehearsalProfile`, with no
arguments or stdin commands. The count is always twelve; `-Trials` is rejected
in this mode. It calls the existing complete snapshot collector twelve times in
one process, preserving every normal identity/state check and query order.
There is no credential retrieval, input, screen-power operation, UiAutomation
connection, or capability-changing call.

One atomic successful UTF-8 response is framed exactly as follows:

```text
REHEARSAL_BEGIN 12
SNAPSHOT 0 <durationMicros>
<normal complete @@identity ... @@end snapshot>
SNAPSHOT 1 <durationMicros>
<normal complete @@identity ... @@end snapshot>
<indices 2 through 10, each with its complete snapshot>
SNAPSHOT 11 <durationMicros>
<normal complete @@identity ... @@end snapshot>
REHEARSAL_TOTAL_NANOS <collectionNanos>
REHEARSAL_END
```

Indices are zero-based and consecutive. Each duration measures only its snapshot
collection call, including the existing diagnostic and identity reads. The total
is the sum of those twelve nanosecond durations; it excludes reflection setup,
framing, UTF-8 encoding, and stdout transfer. Per-snapshot microseconds are rounded
down, so the total is at least their sum times 1,000 and less than that sum plus
12,000 nanoseconds. This is a read-only diagnostic-cost rehearsal, not password,
PIN, or pattern playback and not an unlock/readiness authorization.

The native caller must validate all twelve complete snapshots using its normal
selected-device parser, check the framing/count/index/duration bounds, and retain
its own bounded process timeout and cleanup. It must not infer validity from a
timing row or from collection success alone.

Both new modes share test-only `ProfileSupport`: a separate 2,500 ms watchdog,
fixed `PROFILE_UNAVAILABLE` failure output, and a maximum atomic response of
262,144 UTF-8 bytes. Every service still passes through the production collector's
4 MiB/service and line/filter limits. No successful partial response is emitted.
The build runs all 117 normal helper checks and an unsupported-host fixed-failure
probe; it verifies normal source, resource, and receipt hashes remain unchanged.
The separate profile support/class is never copied into the embedded helper.

Native opt-in tests are `unlock_detection::tests::live_ime_priority_profile_without_input`
and `unlock_detection::tests::live_guard_rehearsal_without_input`. Use the existing
development environment and explicitly select the current ADB executable and
fresh transport ID through `SCRCPY_PLUS_TEST_ADB` and `SCRCPY_PLUS_TEST_TRANSPORT`.
Set `SCRCPY_PLUS_TEST_PROFILE_JAR` to the corresponding generated JAR and
`SCRCPY_PLUS_TEST_PROFILE_SHA256` to its verified build-receipt hash, then run the
selected test with `cargo test --lib --locked <full-test-name> -- --ignored --exact
--nocapture` from `application/src-tauri`. Never run a general `--ignored` sweep:
other ignored tests change phone power or perform an enrolled unlock.

Both callers pin/verify the local JAR and pushed copy, use a unique temporary
phone path, and remove only that file after fresh target verification. They
validate the snapshots with the normal native parser and print only aggregate
diagnostic timings. Rehearsal additionally starts a normal persistent read-only
helper and measures twelve host-mediated snapshots; that helper is also owned
and cleaned up. Phone totals exclude startup and output transfer; host totals
include transport, host polling/parsing, and collection. Do not interpret their
difference as a proven full-unlock speedup. Neither caller accesses enrollment
secrets, writes input/power, registers accessibility services, or closes mirrors.
