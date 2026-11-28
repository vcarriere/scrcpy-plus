# Read-only unlock-state helper

This persistent shell-UID Java process collects the same filtered diagnostic
schema as `application/src-tauri/src/unlock_detection.rs` without starting
`dumpsys`, `getprop`, `cmd`, `awk`, `grep`, or `sed` for every request. Its Java
format follows the existing [scrcpy server](https://github.com/Genymobile/scrcpy/tree/v5.0.1/server/src/main/java/com/genymobile/scrcpy)
reflection wrappers. The helper source is original project code under the
repository's license. Android implementation details come from the
[Android Open Source Project](https://android.googlesource.com/platform/frameworks/base/).

The helper never accepts a password, sends input, authenticates, reads stored
credentials, changes power state, or writes diagnostic files. The Rust caller
continues to own transport binding, identity verification, state parsing,
credential access, cancellation, and process cleanup. A valid diagnostic response
does not itself authorize unlocking. Unsupported or ambiguous states stay closed.

## Protocol

Launch on the explicitly verified target using Android `app_process`, with this
JAR as its `CLASSPATH`, entry point `com.scrcpyplus.unlockguard.Main`, no arguments,
and stdin/stdout pipes. Do not allocate a PTY. The native caller supplies the
exact verified device transport and JAR bytes; this build script never contacts
a phone or runs ADB. The protocol uses explicit file descriptors 0 and 1, since
Android's RuntimeInit can redirect Java's `System` streams to logcat. Framework
`System.out`/`System.err` writes remain discarded.

Each request is ASCII `SNAPSHOT <nonce>\n`. The nonce is 1–64 ASCII letters,
digits, underscores, or hyphens. LF is required. No startup banner is emitted.

Success is one flushed response:

```text
BEGIN <nonce>
@@identity
<serial, manufacturer, product, current user, SDK; one field per line>
@@boot
<sys.boot_completed>
@@policy
<filtered window policy>
@@displays
<display IDs, current focus, and current mRotation metadata>
@@focus_display
<one nonnegative focused display ID per matching current input-service field>
@@ime
<current IME owner/repository/editor metadata and endEditor boundaries>
@@method
<filtered User State credential type metadata>
@@power
<main symbolic wakefulness>
@@panel
<display/powerMode metadata>
@@identity_end
<the same five fresh identity fields>
@@end
END <nonce>
```

The seven Binder queries run sequentially, between fresh identity reads. A changed
identity, missing service, empty filtered service result, reflection failure,
invalid UTF-8/control data, or exceeded bound produces only
`ERROR <nonce> unavailable\n` and process termination. A 2.5-second helper deadline
produces only `ERROR <nonce> timeout\n` and terminates the process. Invalid requests
terminate without a response. Full dump strings, exceptions, and framework
diagnostics are never written to stdout/stderr. Bounds are 4 MiB of streamed
input per service, 16,384 characters per line, and 65,536 characters per snapshot;
the native caller must also bound bytes, elapsed time, and process lifetime.

`input_method` is streamed and filtered on the phone. Input history, surrounding
text, and editor contents are excluded. Existing `awk`/`sed` selection and
boundary semantics are preserved, including duplicate/malformed metadata which
the native parser rejects. `ActivityManager`'s Binder interface is queried
directly for each current-user read, avoiding an application-level cached ID.

Power remains in its original position after lock-method metadata and before the
final SurfaceFlinger panel query. It now reads the bounded Power `--proto` dump
and consumes the complete message and joins its worker before using the root
wakefulness enum. Explicit values 0–3 map to the original symbolic `mWakefulness`
lines without changing the snapshot schema. The one shared `PowerProto` decoder
rejects duplicate root state, incorrect wire type, invalid enums, truncation,
invalid framing, and data exceeding 4 MiB. No state or diagnostic buffer is
cached, and no Power work runs concurrently with other service queries.

Android omits the default single enum0, so a valid absent root field triggers
exactly one fresh text Power query inside the same helper/Binder, validated as
one recognized symbolic state. Absence never infers awake or asleep. Protobuf
parse, I/O, worker, or unsupported-runtime failures stop the request without a
format fallback. This preserves authoritative initial asleep checks and keeps
the complete Power phase before the final panel and identity checks. The
[separate profiling notes](profile/README.md) link the verified Android 15–17
schema and text/protobuf state source.

The displays section also retains each line whose trimmed value starts exactly
`mRotation=`, under its existing display-ID framing. Android 15–17
`DisplayRotation.dump` emits the current value as
`mRotation=<int> mDeferredRotationPauseCount=<int>`; its historical rotation
records use different fields and are excluded. Duplicate/malformed current
rotation lines remain visible to the native parser, which must require exactly
one recognized main-display rotation before using any coordinate-dependent
control. The helper neither chooses coordinates nor sends input. This field is
defined by AOSP's
[DisplayRotation dump](https://android.googlesource.com/platform/frameworks/base/+/refs/tags/android-17.0.0_r1/services/core/java/com/android/server/wm/DisplayRotation.java).

Focused display state is also fresh for every snapshot. Official Android 15–17
`IWindowManager` and `IInputManager` AIDL expose no focused-display getter;
`getTopFocusedDisplayId` belongs to the system-only WindowManager local service.
The helper therefore streams a bounded `input` Binder dump, selecting only exact
`  FocusedDisplayId: [0-9]+` lines inside the exact `Input Dispatcher State:`
section and emitting their numeric values. The next unindented header ends that
section, including the explicitly historical `Input Dispatcher State at time of
last ANR:` section. Missing/malformed current fields or repeated current-section
headers fail closed; duplicate current fields remain duplicated for the native
parser to reject. No focus scalar is cached. The field and both section headers
are defined by AOSP's
[InputDispatcher diagnostic output](https://android.googlesource.com/platform/frameworks/native/+/refs/tags/android-17.0.0_r1/services/inputflinger/dispatcher/InputDispatcher.cpp).

The serial fallback can use the same framing and fail-closed selection:

```sh
dumpsys input | awk '
    /^Input Dispatcher State:$/ {
        if (++sections > 1) { print "invalid"; active=0; next }
        active=1; next
    }
    active && /^[^[:space:]]/ { active=0 }
    active && /^  FocusedDisplayId:/ {
        if ($0 ~ /^  FocusedDisplayId: [0-9]+$/) {
            sub(/^  FocusedDisplayId: /, ""); print
        } else { print "invalid" }
    }
'
```

## Rebuild

```powershell
./buildtools/unlock-guard/Build.ps1 -Bootstrap
./buildtools/unlock-guard/Build.ps1
```

The first command downloads only the pinned official portable Windows x64 JDK
and standalone R8/D8 compiler into `.tools/unlock-guard`. Downloads have bounded
sizes/times and are SHA-256 checked against the publisher values recorded in
`toolchain.json`. The script changes no global Java installation or PATH and
does not install the Android SDK. An explicit `-JavaHome` supports an already
available compatible compiler; the build receipt records the compiler actually
used rather than claiming it used the default pin.

OpenJDK 27.0.1.1 and stable R8/D8 9.5.33 were checked against official release
metadata on November 26, 2026. Alpha R8 versions and early-access JDKs were
excluded. Java 8 bytecode is a concrete Android compatibility pin: an Android 17
read-only probe rejected Java 11 nestmate access from the anonymous Binder worker
with `IllegalAccessError` when D8 desugaring was disabled. `--release 8` generates
synthetic accessors for all nested private accesses instead. Inline string
concatenation and D8's `--no-desugaring` avoid extra runtime libraries or an
Android SDK compilation dependency. The helper uses only Java APIs present on
Android API 35 and Android APIs reached through reflection.

The build runs 51 helper, 39 strict protobuf/text parser, and 27 Power
mapping/freshness/fallback host checks, including an actual-entry-point subprocess
with deliberately redirected Java streams. It compiles all production sources
with `--release 8` and converts only production classes into one `classes.dex`;
tests and profiling classes are excluded. It verifies the final
archive structure and writes `application/src-tauri/resources/unlock-guard.jar`
plus `build-receipt.json`, which records each production source path and hash.
It does not establish real-device compatibility or
latency; read-only target benchmarking belongs to the native integration.

## Build tool licenses

The tools are build-only and are not included in the resulting DEX JAR. Complete
upstream JDK notices remain in the extracted JDK's `legal/` directory. The JDK
license, Classpath Exception, assembly exception, and additional license
information are also retained in `licenses/`. R8's BSD license and complete
bundled third-party license text from the pinned Maven JAR are retained there.
The R8 source and official prebuilt instructions are available at
[r8.googlesource.com](https://r8.googlesource.com/r8/).
