package com.scrcpyplus.unlockguard;

import java.util.concurrent.TimeUnit;

/** Test-only normal/critical IME comparison. No raw IME output or files. */
public final class MainImeCriticalProfile {
    static final int TRIALS = 5;

    private MainImeCriticalProfile() {
        /* not instantiable */
    }

    public static void main(String[] args) {
        ProfileSupport.run(args, "unlock-guard-ime-profile", new ProfileSupport.Operation() {
            @Override
            public String collect() throws Exception {
                ProfileSupport.Android android = new ProfileSupport.Android();
                String before = android.snapshot();
                StringBuilder timings = new StringBuilder();
                for (int trial = 0; trial < TRIALS; ++trial) {
                    Sample normal;
                    Sample critical;
                    if (trial % 2 == 0) {
                        normal = measure(android, false);
                        critical = measure(android, true);
                    } else {
                        critical = measure(android, true);
                        normal = measure(android, false);
                    }
                    timings.append("ime_normal=").append(normal.micros).append('\n');
                    timings.append("ime_critical=").append(critical.micros).append('\n');
                    timings.append("ime_equal=").append(normal.filtered.equals(critical.filtered) ? 1 : 0).append('\n');
                }
                String after = android.snapshot();
                return "PROFILE_BEFORE\n" + before + "PROFILE_TIMINGS\n" + timings + "PROFILE_AFTER\n" + after;
            }
        });
    }

    private static Sample measure(ProfileSupport.Android android, boolean critical) throws Exception {
        long started = System.nanoTime();
        String filtered = android.ime(critical);
        long micros = TimeUnit.NANOSECONDS.toMicros(System.nanoTime() - started);
        return new Sample(micros, filtered);
    }

    private static final class Sample {
        final long micros;
        final String filtered;

        Sample(long micros, String filtered) {
            this.micros = micros;
            this.filtered = filtered;
        }
    }
}
