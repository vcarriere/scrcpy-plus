package com.scrcpyplus.unlockguard;

import java.io.IOException;
import java.io.InputStream;
import java.lang.reflect.Constructor;
import java.lang.reflect.Method;
import java.util.concurrent.atomic.AtomicReference;

/** Fresh bounded Power query, preserving the original symbolic snapshot schema. */
final class PowerDump {
    private static final String[] NAMES = {"Asleep", "Awake", "Dreaming", "Dozing"};

    private final Method service;
    private final Method dump;
    private final Method createPipe;
    private final Method descriptor;
    private final Method close;
    private final Constructor<?> input;

    PowerDump(Method service, Method dump, Method createPipe, Method descriptor, Method close, Constructor<?> input) {
        this.service = service;
        this.dump = dump;
        this.createPipe = createPipe;
        this.descriptor = descriptor;
        this.close = close;
        this.input = input;
    }

    interface ProtoRead {
        int read() throws Exception;
    }

    interface TextRead {
        String read() throws Exception;
    }

    static String current(ProtoRead proto, TextRead text) throws Exception {
        int state = proto.read();
        if (state == PowerProto.ABSENT) {
            // ProtoOutputStream omits the default enum0. Confirm the current
            // symbolic state with a fresh text query rather than infer a value.
            String fresh = text.read();
            textWakefulness(fresh);
            return fresh;
        }
        if (state < 0 || state >= NAMES.length) {
            throw new IOException("Invalid Power state");
        }
        return "@@power\n  mWakefulness=" + NAMES[state] + "\n";
    }

    static int textWakefulness(String filtered) throws IOException {
        for (int state = 0; state < NAMES.length; ++state) {
            if (filtered.equals("@@power\n  mWakefulness=" + NAMES[state] + "\n")) { return state; }
        }
        throw new IOException("Ambiguous text wakefulness");
    }

    int read() throws Exception {
        final Object binder = service.invoke(null, "power");
        if (binder == null) { throw new IOException("Missing Power service"); }
        final Object[] pipe = (Object[]) createPipe.invoke(null);
        final AtomicReference<Throwable> failure = new AtomicReference<>();
        Thread worker = new Thread(new Runnable() {
            @Override
            public void run() {
                try {
                    dump.invoke(binder, descriptor.invoke(pipe[1]), new String[] {"--proto"});
                } catch (Exception | LinkageError e) {
                    failure.set(e);
                } finally {
                    try {
                        close.invoke(pipe[1]);
                    } catch (Exception | LinkageError e) {
                        failure.compareAndSet(null, e);
                    }
                }
            }
        }, "unlock-guard-power-proto-dump");
        worker.setDaemon(true);
        int state;
        try (InputStream stream = (InputStream) input.newInstance(pipe[0])) {
            worker.start();
            state = PowerProto.read(stream);
        } finally {
            if (worker.getState() == Thread.State.NEW) {
                close.invoke(pipe[1]);
                close.invoke(pipe[0]);
            } else {
                // No result or next request can outlive its Power dump worker.
                // The request watchdog remains active while this join waits.
                worker.join(Main.DEADLINE_MS);
            }
        }
        if (worker.isAlive() || failure.get() != null) { throw new IOException("Power proto dump failed"); }
        return state;
    }
}
