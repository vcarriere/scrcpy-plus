package com.scrcpyplus.unlockguard;

import java.util.concurrent.TimeUnit;

/** Test-only bounded snapshot rehearsal. Never retrieves credentials or sends input. */
public final class MainRehearsalProfile {
    static final int SNAPSHOTS = 12;

    private MainRehearsalProfile() {
        /* not instantiable */
    }

    public static void main(String[] args) {
        ProfileSupport.run(args, "unlock-guard-rehearsal-profile", new ProfileSupport.Operation() {
            @Override
            public String collect() throws Exception {
                ProfileSupport.Android android = new ProfileSupport.Android();
                StringBuilder response = new StringBuilder();
                ProfileSupport.append(response, "REHEARSAL_BEGIN " + SNAPSHOTS + "\n");
                long totalNanos = 0;
                for (int index = 0; index < SNAPSHOTS; ++index) {
                    long started = System.nanoTime();
                    String snapshot = android.snapshot();
                    long nanos = System.nanoTime() - started;
                    if (nanos < 0 || Long.MAX_VALUE - totalNanos < nanos) {
                        throw new IllegalStateException("Invalid profile duration");
                    }
                    totalNanos += nanos;
                    ProfileSupport.append(response, "SNAPSHOT " + index + " " + TimeUnit.NANOSECONDS.toMicros(nanos) + "\n");
                    ProfileSupport.append(response, snapshot);
                }
                ProfileSupport.append(response, "REHEARSAL_TOTAL_NANOS " + totalNanos + "\nREHEARSAL_END\n");
                return response.toString();
            }
        });
    }
}
