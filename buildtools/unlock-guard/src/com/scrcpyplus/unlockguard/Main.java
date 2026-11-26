package com.scrcpyplus.unlockguard;

import java.io.BufferedInputStream;
import java.io.BufferedReader;
import java.io.FileDescriptor;
import java.io.FileInputStream;
import java.io.FileOutputStream;
import java.io.IOException;
import java.io.InputStream;
import java.io.InputStreamReader;
import java.io.OutputStream;
import java.io.PrintStream;
import java.lang.reflect.Constructor;
import java.lang.reflect.Field;
import java.lang.reflect.Method;
import java.nio.charset.CodingErrorAction;
import java.nio.charset.StandardCharsets;
import java.util.concurrent.CountDownLatch;
import java.util.concurrent.TimeUnit;
import java.util.concurrent.atomic.AtomicReference;
import java.util.regex.Matcher;
import java.util.regex.Pattern;

/** Read-only, shell-UID diagnostic bridge. All credential decisions remain native. */
public final class Main {

    static final int MAX_REQUEST = 80;
    static final int MAX_LINE = 16384;
    static final int MAX_DUMP_BYTES = 4 * 1024 * 1024;
    static final int MAX_RESPONSE = 65536;
    static final int DEADLINE_MS = 2500;

    private static final Pattern NONCE = Pattern.compile("[A-Za-z0-9_-]{1,64}");
    private static final Object OUTPUT_LOCK = new Object();

    private Main() {
        /* not instantiable */
    }

    public static void main(String[] args) {
        try {
            // RuntimeInit may replace System streams with Android log streams.
            // The protocol must remain on the caller's pipes, never in logcat.
            run(args, new BufferedInputStream(new FileInputStream(FileDescriptor.in)),
                    new PrintStream(new FileOutputStream(FileDescriptor.out), true, "UTF-8"));
        } catch (Exception | LinkageError e) {
            // A missing caller-owned pipe fails closed without diagnostics.
        }
    }

    static void run(String[] args, InputStream input, final PrintStream protocol) {
        // Android diagnostics must never become protocol payload or leak a dump.
        System.setErr(new PrintStream(new OutputStream() {
            @Override
            public void write(int value) {
                // Discard framework diagnostics and exception messages.
            }
        }));
        try {
            if (args.length != 0) {
                return;
            }
            System.setOut(System.err);
            Android android = null;
            String request;
            while ((request = readRequest(input)) != null) {
                if (!request.startsWith("SNAPSHOT ") || !NONCE.matcher(request.substring(9)).matches()) {
                    return;
                }
                final String nonce = request.substring(9);
                final CountDownLatch completed = new CountDownLatch(1);
                Thread watchdog = new Thread(new Runnable() {
                    @Override
                    public void run() {
                        try {
                            if (!completed.await(DEADLINE_MS, TimeUnit.MILLISECONDS)) {
                                synchronized (OUTPUT_LOCK) {
                                    if (completed.getCount() != 0) {
                                        protocol.print("ERROR " + nonce + " timeout\n");
                                        protocol.flush();
                                        Runtime.getRuntime().halt(2);
                                    }
                                }
                            }
                        } catch (InterruptedException e) {
                            Thread.currentThread().interrupt();
                        }
                    }
                }, "unlock-guard-deadline");
                watchdog.setDaemon(true);
                watchdog.start();
                try {
                    if (android == null) {
                        android = new Android();
                    }
                    String response = android.snapshot();
                    synchronized (OUTPUT_LOCK) {
                        protocol.print("BEGIN " + nonce + "\n" + response + "END " + nonce + "\n");
                        protocol.flush();
                        completed.countDown();
                    }
                } catch (Exception | LinkageError e) {
                    synchronized (OUTPUT_LOCK) {
                        protocol.print("ERROR " + nonce + " unavailable\n");
                        protocol.flush();
                        completed.countDown();
                    }
                    return;
                } finally {
                    completed.countDown();
                }
            }
        } catch (Exception | LinkageError e) {
            // Invalid/oversized input and unsupported runtimes fail closed.
        }
    }

    static String readRequest(InputStream input) throws IOException {
        StringBuilder value = new StringBuilder();
        int next;
        while ((next = input.read()) != -1) {
            if (next == '\n') {
                return value.toString();
            }
            if (next < 32 || next > 126 || value.length() == MAX_REQUEST) {
                throw new IOException("Invalid request");
            }
            value.append((char) next);
        }
        if (value.length() != 0) {
            throw new IOException("Incomplete request");
        }
        return null;
    }

    private static final class Android {
        private final Method property;
        private final Method service;
        private final Method dump;
        private final Method createPipe;
        private final Method fileDescriptor;
        private final Method close;
        private final Constructor<?> autoCloseInput;
        private final PowerDump power;
        private final Object activity;
        private final Method currentUser;
        private final Field userId;

        Android() throws Exception {
            property = Class.forName("android.os.SystemProperties").getMethod("get", String.class);
            service = Class.forName("android.os.ServiceManager").getMethod("getService", String.class);
            dump = Class.forName("android.os.IBinder").getMethod("dump", FileDescriptor.class, String[].class);
            Class<?> descriptor = Class.forName("android.os.ParcelFileDescriptor");
            createPipe = descriptor.getMethod("createPipe");
            fileDescriptor = descriptor.getMethod("getFileDescriptor");
            close = descriptor.getMethod("close");
            autoCloseInput = Class.forName("android.os.ParcelFileDescriptor$AutoCloseInputStream").getConstructor(descriptor);
            power = new PowerDump(service, dump, createPipe, fileDescriptor, close, autoCloseInput);
            activity = Class.forName("android.app.ActivityManager").getMethod("getService").invoke(null);
            if (activity == null) {
                throw new IOException("Activity service unavailable");
            }
            Class<?> manager = Class.forName("android.app.IActivityManager");
            Method current;
            Field id = null;
            try {
                // Call the Binder interface directly; do not use a cached user ID.
                current = manager.getMethod("getCurrentUserId");
            } catch (NoSuchMethodException e) {
                current = manager.getMethod("getCurrentUser");
                id = Class.forName("android.content.pm.UserInfo").getField("id");
            }
            currentUser = current;
            userId = id;
        }

        String snapshot() throws Exception {
            StringBuilder output = new StringBuilder();
            append(output, "@@identity\n");
            String before = identity();
            append(output, before);
            append(output, "@@boot\n" + getProperty("sys.boot_completed") + "\n");
            dump(output, "@@policy\n", "window", new String[] {"policy"}, new PolicyFilter());
            dump(output, "@@displays\n", "window", new String[] {"displays"}, new DisplaysFilter());
            dump(output, "@@focus_display\n", "input", new String[0], new FocusDisplayFilter());
            dump(output, "@@ime\n", "input_method", new String[0], new ImeFilter());
            dump(output, "@@method\n", "lock_settings", new String[0], new MethodFilter());
            append(output, power());
            dump(output, "@@panel\n", "SurfaceFlinger", new String[] {"--displays"}, new PanelFilter());
            String after = identity();
            if (!before.equals(after)) {
                throw new IOException("Identity changed");
            }
            append(output, "@@identity_end\n" + after + "@@end\n");
            return output.toString();
        }

        private String power() throws Exception {
            return PowerDump.current(new PowerDump.ProtoRead() {
                @Override
                public int read() throws Exception {
                    return power.read();
                }
            }, new PowerDump.TextRead() {
                @Override
                public String read() throws Exception {
                    StringBuilder filtered = new StringBuilder();
                    dump(filtered, "@@power\n", "power", new String[0], new PowerFilter());
                    return filtered.toString();
                }
            });
        }

        private String getProperty(String name) throws Exception {
            Object value = property.invoke(null, name);
            if (!(value instanceof String) || ((String) value).length() > 256) {
                throw new IOException("Invalid identity property");
            }
            String text = (String) value;
            for (int i = 0; i < text.length(); ++i) {
                if (Character.isISOControl(text.charAt(i))) {
                    throw new IOException("Invalid identity property");
                }
            }
            return text;
        }

        private String identity() throws Exception {
            Object user = currentUser.invoke(activity);
            Object id = userId == null ? user : userId.get(user);
            if (!(id instanceof Integer) || (Integer) id < 0) {
                throw new IOException("Invalid Android user");
            }
            String serial = getProperty("ro.serialno");
            String manufacturer = getProperty("ro.product.manufacturer");
            String product = getProperty("ro.product.device");
            String sdk = getProperty("ro.build.version.sdk");
            if (serial.isEmpty() || manufacturer.isEmpty() || product.isEmpty() || !sdk.matches("[0-9]{1,3}")) {
                throw new IOException("Incomplete identity");
            }
            return serial + "\n" + manufacturer + "\n" + product + "\n" + id + "\n" + sdk + "\n";
        }

        private void dump(StringBuilder output, String marker, String name, final String[] arguments, Filter filter) throws Exception {
            final Object binder = service.invoke(null, name);
            if (binder == null) {
                throw new IOException("Service unavailable");
            }
            final Object[] descriptors = (Object[]) createPipe.invoke(null);
            final Object write = descriptors[1];
            final AtomicReference<Throwable> failure = new AtomicReference<>();
            Thread worker = new Thread(new Runnable() {
                @Override
                public void run() {
                    try {
                        dump.invoke(binder, fileDescriptor.invoke(write), arguments);
                    } catch (Exception | LinkageError e) {
                        failure.set(e);
                    } finally {
                        try {
                            close.invoke(write);
                        } catch (Exception e) {
                            failure.compareAndSet(null, e);
                        }
                    }
                }
            }, "unlock-guard-dump");
            worker.setDaemon(true);
            try (InputStream input = (InputStream) autoCloseInput.newInstance(descriptors[0])) {
                worker.start();
                append(output, marker);
                int beginning = output.length();
                filter(input, output, filter);
                worker.join();
                if (failure.get() != null || output.length() == beginning) {
                    throw new IOException("Service dump unavailable");
                }
            } finally {
                if (worker.getState() == Thread.State.NEW) {
                    close.invoke(write);
                    close.invoke(descriptors[0]);
                }
            }
        }
    }

    static void filter(InputStream input, StringBuilder output, Filter filter) throws IOException {
        InputStream limited = new InputStream() {
            private int count;

            @Override
            public int read() throws IOException {
                int next = input.read();
                if (next != -1 && ++count > MAX_DUMP_BYTES) {
                    throw new IOException("Oversized dump");
                }
                return next;
            }

            @Override
            public int read(byte[] bytes, int offset, int length) throws IOException {
                int read = input.read(bytes, offset, Math.min(length, MAX_DUMP_BYTES - count + 1));
                if (read > 0 && (count += read) > MAX_DUMP_BYTES) {
                    throw new IOException("Oversized dump");
                }
                return read;
            }
        };
        BufferedReader reader = new BufferedReader(new InputStreamReader(limited, StandardCharsets.UTF_8.newDecoder()
                .onMalformedInput(CodingErrorAction.REPORT).onUnmappableCharacter(CodingErrorAction.REPORT)), 4096);
        StringBuilder line = new StringBuilder();
        int next;
        while ((next = reader.read()) != -1) {
            if (next == '\n') {
                filter.line(line.toString(), output);
                line.setLength(0);
            } else {
                if (line.length() == MAX_LINE || next == 0 || next == '\r') {
                    throw new IOException("Invalid diagnostic line");
                }
                line.append((char) next);
            }
        }
        if (line.length() != 0) {
            filter.line(line.toString(), output);
        }
    }

    static void append(StringBuilder output, String text) throws IOException {
        if (output.length() + text.length() > MAX_RESPONSE) {
            throw new IOException("Oversized response");
        }
        output.append(text);
    }

    interface Filter {
        void line(String line, StringBuilder output) throws IOException;
    }

    static final class PolicyFilter implements Filter {
        private static final Pattern HEADER = Pattern.compile("^\\s*KeyguardServiceDelegate\\s*$");
        private static final Pattern END = Pattern.compile("^\\s*Looper state:");
        private static final Pattern FIELD = Pattern.compile("^\\s*(showing|inputRestricted|userId|simSecure|occluded|bootCompleted|interactiveState)=");
        private boolean active;

        @Override
        public void line(String line, StringBuilder output) throws IOException {
            if (HEADER.matcher(line).find()) {
                active = true;
                append(output, line + "\n");
            } else {
                if (END.matcher(line).find()) {
                    active = false;
                }
                if (active && FIELD.matcher(line).find()) {
                    append(output, line + "\n");
                }
            }
        }
    }

    static final class DisplaysFilter implements Filter {
        @Override
        public void line(String line, StringBuilder output) throws IOException {
            if (line.contains("Display: mDisplayId=") || line.contains("mCurrentFocus=") || line.trim().startsWith("mRotation=")) {
                append(output, line + "\n");
            }
        }
    }

    static final class FocusDisplayFilter implements Filter {
        private static final Pattern FIELD = Pattern.compile("^  FocusedDisplayId: ([0-9]+)$");
        private static final Pattern END = Pattern.compile("^\\S");
        private boolean active;
        private int sections;

        @Override
        public void line(String line, StringBuilder output) throws IOException {
            if (line.equals("Input Dispatcher State:")) {
                if (++sections != 1) {
                    throw new IOException("Duplicate current input state");
                }
                active = true;
                return;
            }
            if (END.matcher(line).find()) {
                active = false;
            }
            if (active && line.startsWith("  FocusedDisplayId:")) {
                Matcher match = FIELD.matcher(line);
                if (!match.matches()) {
                    throw new IOException("Invalid focused display state");
                }
                // Preserve duplicate current fields for native ambiguity checks.
                append(output, match.group(1) + "\n");
            }
        }
    }

    static final class ImeFilter implements Filter {
        private boolean repository;

        @Override
        public void line(String line, StringBuilder output) throws IOException {
            // Match both sed -n expressions independently, including range ends.
            if (line.startsWith("  mCurrentImeUserId=")) {
                selected(line, output);
            }
            if (!repository && line.equals("  mUserDataRepository:")) {
                repository = true;
                selected(line, output);
            } else if (repository) {
                selected(line, output);
                if (line.length() > 2 && line.startsWith("  ") && line.charAt(2) != ' ') {
                    repository = false;
                }
            }
        }

        private void selected(String line, StringBuilder output) throws IOException {
            if (line.startsWith("  mCurrentImeUserId=") || line.equals("  mUserDataRepository:") || line.startsWith("    userId=")
                    || line.startsWith("      curClient=") || line.equals("      curEditorInfo:") || line.startsWith("        inputType=")
                    || line.startsWith("        packageName=")) {
                append(output, line + "\n");
            } else if (line.length() > 6 && line.startsWith("      ") && line.charAt(6) != ' ') {
                append(output, "      endEditor\n");
            }
        }
    }

    static final class MethodFilter implements Filter {
        private static final Pattern FIELD = Pattern.compile("^\\s+(User|CredentialType:|IsLockScreenDisabled:)");
        private boolean active;

        @Override
        public void line(String line, StringBuilder output) throws IOException {
            if (line.equals("User State:")) {
                active = true;
                append(output, line + "\n");
                return;
            }
            if (!line.isEmpty() && !Character.isWhitespace(line.charAt(0))) {
                active = false;
            }
            if (active && FIELD.matcher(line).find()) {
                append(output, line + "\n");
            }
        }
    }

    static final class PowerFilter implements Filter {
        private static final Pattern FIELD = Pattern.compile("^  mWakefulness=[A-Za-z]*$");

        @Override
        public void line(String line, StringBuilder output) throws IOException {
            if (FIELD.matcher(line).matches()) {
                append(output, line + "\n");
            }
        }
    }

    static final class PanelFilter implements Filter {
        private static final Pattern FIELD = Pattern.compile("^\\s*((Virtual )?Display [0-9]|powerMode=)");

        @Override
        public void line(String line, StringBuilder output) throws IOException {
            if (FIELD.matcher(line).find()) {
                append(output, line + "\n");
            }
        }
    }
}
