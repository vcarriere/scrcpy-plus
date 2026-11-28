package com.scrcpyplus.unlockguard;

import java.io.FileDescriptor;
import java.io.FileOutputStream;
import java.io.IOException;
import java.io.OutputStream;
import java.io.PrintStream;
import java.lang.reflect.Constructor;
import java.lang.reflect.Method;
import java.nio.charset.StandardCharsets;
import java.util.concurrent.CountDownLatch;
import java.util.concurrent.TimeUnit;

/** Bounds and reflection shared only by the separate diagnostic profilers. */
final class ProfileSupport {
    static final int MAX_RESPONSE = 262144;
    private static final Object OUTPUT_LOCK = new Object();

    private ProfileSupport() {
        /* not instantiable */
    }

    interface Operation {
        String collect() throws Exception;
    }

    static void run(String[] args, final String name, Operation operation) {
        try {
            final PrintStream output = new PrintStream(new FileOutputStream(FileDescriptor.out), true, "UTF-8");
            PrintStream discard = new PrintStream(new OutputStream() {
                @Override
                public void write(int value) {
                    // No framework diagnostics or exception values leave the profiler.
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
            }, name + "-deadline");
            watchdog.setDaemon(true);
            watchdog.start();
            try {
                if (args.length != 0) {
                    throw new IOException("Unsupported arguments");
                }
                byte[] response = encode(operation.collect());
                synchronized (OUTPUT_LOCK) {
                    output.write(response, 0, response.length);
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

    static byte[] encode(String response) throws IOException {
        if (response == null || response.length() > MAX_RESPONSE) {
            throw new IOException("Oversized profile");
        }
        byte[] bytes = response.getBytes(StandardCharsets.UTF_8);
        if (bytes.length > MAX_RESPONSE) {
            throw new IOException("Oversized profile");
        }
        return bytes;
    }

    static void append(StringBuilder output, String value) throws IOException {
        if (value == null || value.length() > MAX_RESPONSE - output.length()) {
            throw new IOException("Oversized profile");
        }
        output.append(value);
        encode(output.toString());
    }

    static final class Android {
        private final Object android;
        private final Method snapshot;
        private final Method dump;

        Android() throws Exception {
            Class<?> type = Class.forName("com.scrcpyplus.unlockguard.Main$Android");
            Constructor<?> constructor = type.getDeclaredConstructor();
            constructor.setAccessible(true);
            android = constructor.newInstance();
            snapshot = type.getDeclaredMethod("snapshot");
            dump = type.getDeclaredMethod("dump", StringBuilder.class, String.class, String.class, String[].class, Main.Filter.class);
            snapshot.setAccessible(true);
            dump.setAccessible(true);
        }

        String snapshot() throws Exception {
            return (String) snapshot.invoke(android);
        }

        String ime(boolean critical) throws Exception {
            StringBuilder output = new StringBuilder();
            dump.invoke(android, output, "@@ime\n", "input_method",
                    critical ? new String[] {"--dump-priority", "CRITICAL"} : new String[0], new Main.ImeFilter());
            return output.toString();
        }
    }
}
