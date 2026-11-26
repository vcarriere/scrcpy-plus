package com.scrcpyplus.unlockguard;

import java.io.FileDescriptor;
import java.io.FileOutputStream;
import java.io.OutputStream;
import java.io.PrintStream;
import java.lang.reflect.Constructor;
import java.lang.reflect.Method;
import java.util.concurrent.CountDownLatch;
import java.util.concurrent.TimeUnit;

/** Test-only sequential diagnostic profiler. Never packaged in the normal helper. */
public final class MainProfile {

    static final int TRIALS = 5;
    private static final Object OUTPUT_LOCK = new Object();

    private MainProfile() {
        /* not instantiable */
    }

    public static void main(String[] args) {
        try {
            final PrintStream output = new PrintStream(new FileOutputStream(FileDescriptor.out), true, "UTF-8");
            PrintStream discard = new PrintStream(new OutputStream() {
                @Override
                public void write(int value) {
                    // Do not expose framework diagnostics or exception values.
                }
            });
            System.setOut(discard);
            System.setErr(discard);
            final CountDownLatch completed = new CountDownLatch(1);
            Thread watchdog = new Thread(new Runnable() {
                @Override
                public void run() {
                    try {
                        if (!completed.await(2500, TimeUnit.MILLISECONDS)) {
                            synchronized (OUTPUT_LOCK) {
                                if (completed.getCount() != 0) {
                                    output.print("PROFILE_UNAVAILABLE\n");
                                    output.flush();
                                    Runtime.getRuntime().halt(2);
                                }
                            }
                        }
                    } catch (InterruptedException e) {
                        Thread.currentThread().interrupt();
                    }
                }
            }, "unlock-guard-profile-deadline");
            watchdog.setDaemon(true);
            watchdog.start();
            try {
                if (args.length != 0) {
                    throw new IllegalArgumentException("Unsupported arguments");
                }
                Class<?> type = Class.forName("com.scrcpyplus.unlockguard.Main$Android");
                Constructor<?> constructor = type.getDeclaredConstructor();
                constructor.setAccessible(true);
                Object android = constructor.newInstance();
                Method snapshot = type.getDeclaredMethod("snapshot");
                Method dump = type.getDeclaredMethod("dump", StringBuilder.class, String.class, String.class, String[].class, Main.Filter.class);
                snapshot.setAccessible(true);
                dump.setAccessible(true);
                String before = (String) snapshot.invoke(android);
                StringBuilder timings = new StringBuilder();
                for (int trial = 0; trial < TRIALS; ++trial) {
                    measure(android, dump, timings, "policy", "window", new String[] {"policy"}, new Main.PolicyFilter());
                    measure(android, dump, timings, "displays", "window", new String[] {"displays"}, new Main.DisplaysFilter());
                    measure(android, dump, timings, "focus_display", "input", new String[0], new Main.FocusDisplayFilter());
                    measure(android, dump, timings, "input_method", "input_method", new String[0], new Main.ImeFilter());
                    measure(android, dump, timings, "lock_settings", "lock_settings", new String[0], new Main.MethodFilter());
                    measure(android, dump, timings, "power", "power", new String[0], new Main.PowerFilter());
                    measure(android, dump, timings, "SurfaceFlinger", "SurfaceFlinger", new String[] {"--displays"}, new Main.PanelFilter());
                }
                String after = (String) snapshot.invoke(android);
                String response = "PROFILE_BEFORE\n" + before + "PROFILE_TIMINGS\n" + timings + "PROFILE_AFTER\n" + after;
                if (response.length() > 2 * Main.MAX_RESPONSE + 4096) {
                    throw new IllegalStateException("Oversized profile response");
                }
                synchronized (OUTPUT_LOCK) {
                    output.print(response);
                    output.flush();
                    completed.countDown();
                }
            } catch (Exception | LinkageError e) {
                synchronized (OUTPUT_LOCK) {
                    output.print("PROFILE_UNAVAILABLE\n");
                    output.flush();
                    completed.countDown();
                }
            } finally {
                completed.countDown();
            }
        } catch (Exception | LinkageError e) {
            // If fd1 is unavailable, no safe diagnostic output is possible.
        }
    }

    private static void measure(Object android, Method dump, StringBuilder timings, String label, String service,
            String[] arguments, Main.Filter filter) throws Exception {
        StringBuilder filtered = new StringBuilder();
        long started = System.nanoTime();
        dump.invoke(android, filtered, "@@" + label + "\n", service, arguments, filter);
        long elapsed = TimeUnit.NANOSECONDS.toMicros(System.nanoTime() - started);
        timings.append(label).append('=').append(elapsed).append('\n');
        // Filtered diagnostic values are deliberately not included in timings.
    }
}
