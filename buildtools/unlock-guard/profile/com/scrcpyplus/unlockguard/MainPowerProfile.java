package com.scrcpyplus.unlockguard;

import java.io.FileDescriptor;
import java.io.FileOutputStream;
import java.io.IOException;
import java.io.OutputStream;
import java.io.PrintStream;
import java.lang.reflect.Constructor;
import java.lang.reflect.Field;
import java.lang.reflect.Method;
import java.util.concurrent.CountDownLatch;
import java.util.concurrent.TimeUnit;

/** Test-only text/proto Power comparison. No raw protobuf output or files. */
public final class MainPowerProfile {
    static final int TRIALS = 5;
    private static final Object OUTPUT_LOCK = new Object();

    private MainPowerProfile() {
        /* not instantiable */
    }

    public static void main(String[] args) {
        try {
            final PrintStream output = new PrintStream(new FileOutputStream(FileDescriptor.out), true, "UTF-8");
            PrintStream discard = new PrintStream(new OutputStream() {
                @Override
                public void write(int value) {
                    // Discard diagnostics, exception values, and framework logs.
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
            }, "unlock-guard-power-profile-deadline");
            watchdog.setDaemon(true);
            watchdog.start();
            try {
                if (args.length != 0) { throw new IOException("Unsupported arguments"); }
                Class<?> type = Class.forName("com.scrcpyplus.unlockguard.Main$Android");
                Constructor<?> constructor = type.getDeclaredConstructor();
                constructor.setAccessible(true);
                Object android = constructor.newInstance();
                Method snapshot = type.getDeclaredMethod("snapshot");
                Method dump = type.getDeclaredMethod("dump", StringBuilder.class, String.class, String.class, String[].class, Main.Filter.class);
                snapshot.setAccessible(true);
                dump.setAccessible(true);
                PowerDump binary = new PowerDump((Method) member(type, android, "service"), (Method) member(type, android, "dump"),
                        (Method) member(type, android, "createPipe"), (Method) member(type, android, "fileDescriptor"),
                        (Method) member(type, android, "close"), (Constructor<?>) member(type, android, "autoCloseInput"));
                String before = (String) snapshot.invoke(android);
                StringBuilder timings = new StringBuilder();
                for (int trial = 0; trial < TRIALS; ++trial) {
                    Sample text;
                    Sample proto;
                    if (trial % 2 == 0) {
                        text = text(android, dump);
                        proto = proto(binary);
                    } else {
                        proto = proto(binary);
                        text = text(android, dump);
                    }
                    timings.append("power_text=").append(text.micros).append('\n');
                    timings.append("power_text_wakefulness=").append(text.wakefulness).append('\n');
                    timings.append("power_proto=").append(proto.micros).append('\n');
                    timings.append("power_proto_wakefulness=").append(proto.wakefulness).append('\n');
                }
                String after = (String) snapshot.invoke(android);
                String response = "PROFILE_BEFORE\n" + before + "PROFILE_TIMINGS\n" + timings + "PROFILE_AFTER\n" + after;
                if (response.length() > 2 * Main.MAX_RESPONSE + 4096) { throw new IOException("Oversized profile"); }
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
            // No safe output is possible without the caller's fd1.
        }
    }

    private static Sample text(Object android, Method dump) throws Exception {
        StringBuilder filtered = new StringBuilder();
        long started = System.nanoTime();
        dump.invoke(android, filtered, "@@power\n", "power", new String[0], new Main.PowerFilter());
        int state = textWakefulness(filtered.toString());
        return new Sample(TimeUnit.NANOSECONDS.toMicros(System.nanoTime() - started), state);
    }

    static int textWakefulness(String filtered) throws IOException {
        return PowerDump.textWakefulness(filtered);
    }

    private static Sample proto(PowerDump dump) throws Exception {
        long started = System.nanoTime();
        int state = dump.read();
        return new Sample(TimeUnit.NANOSECONDS.toMicros(System.nanoTime() - started), state);
    }

    private static final class Sample {
        final long micros;
        final int wakefulness;

        Sample(long micros, int wakefulness) {
            this.micros = micros;
            this.wakefulness = wakefulness;
        }
    }

    private static Object member(Class<?> type, Object instance, String name) throws Exception {
        Field field = type.getDeclaredField(name);
        field.setAccessible(true);
        return field.get(instance);
    }
}
