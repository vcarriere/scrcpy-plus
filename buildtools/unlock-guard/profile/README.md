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
