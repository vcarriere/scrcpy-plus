package com.scrcpyplus.unlockguard;

import java.io.ByteArrayInputStream;
import java.io.ByteArrayOutputStream;
import java.io.IOException;
import java.io.InputStream;
import java.io.PrintStream;
import java.nio.charset.StandardCharsets;
import java.nio.file.Paths;
import java.util.concurrent.TimeUnit;

public final class MainTest {

    private static int checks;

    private MainTest() {
        /* not instantiable */
    }

    public static void main(String[] args) throws Exception {
        if (args.length == 1 && args[0].equals("redirected-system-streams")) {
            // Emulate RuntimeInit stream replacement before the actual entry point.
            System.setIn(bytes(""));
            PrintStream discard = new PrintStream(new ByteArrayOutputStream());
            System.setOut(discard);
            System.setErr(discard);
            Main.main(new String[0]);
            return;
        }
        matches(new Main.PolicyFilter(), "showing=false\n  KeyguardServiceDelegate\n    showing=true\n    inputRestricted=true\n"
                + "    userId=0\n    simSecure=false\n    occluded=false\n    bootCompleted=true\n"
                + "    interactiveState=INTERACTIVE_STATE_AWAKE\n    unrelated=private\n    Looper state: secret\n    showing=false\n",
                "  KeyguardServiceDelegate\n    showing=true\n    inputRestricted=true\n    userId=0\n    simSecure=false\n"
                + "    occluded=false\n    bootCompleted=true\n    interactiveState=INTERACTIVE_STATE_AWAKE\n");
        matches(new Main.DisplaysFilter(), "    Display: mDisplayId=1\n      mCurrentFocus=Window{other}\n"
                + "    mRotation=2 mDeferredRotationPauseCount=0\nprivate\n"
                + "Display: mDisplayId=0\n  mCurrentFocus=Window{keyguard u0 NotificationShade}\n"
                + "    mRotation=0 mDeferredRotationPauseCount=0\n",
                "    Display: mDisplayId=1\n      mCurrentFocus=Window{other}\n    mRotation=2 mDeferredRotationPauseCount=0\n"
                + "Display: mDisplayId=0\n  mCurrentFocus=Window{keyguard u0 NotificationShade}\n"
                + "    mRotation=0 mDeferredRotationPauseCount=0\n");
        matches(new Main.DisplaysFilter(), "Display: mDisplayId=0\n    mRotation=0 mDeferredRotationPauseCount=0\n"
                + "    mRotation=0 mDeferredRotationPauseCount=0\n    mRotation=unknown\n",
                "Display: mDisplayId=0\n    mRotation=0 mDeferredRotationPauseCount=0\n"
                + "    mRotation=0 mDeferredRotationPauseCount=0\n    mRotation=unknown\n");
        matches(new Main.DisplaysFilter(), "    DisplayRotation\n    mCurrentAppOrientation=SCREEN_ORIENTATION_UNSPECIFIED\n"
                + "    mRotationHistory=private\n    RotationHistory:\n      12:00:00 ROTATION_90 to ROTATION_0\n"
                + "        source=private\n        mode=USER_ROTATION_FREE user=ROTATION_0\n"
                + "    rotation=ROTATION_0\n    metadata mRotation=0\n", "");
        matches(new Main.FocusDisplayFilter(), "Input Dispatcher State:\n  FocusedDisplayId: 0\n"
                + "    FocusedDisplayId: 12\n  private=secret\n", "0\n");
        matches(new Main.FocusDisplayFilter(), "Input Dispatcher State:\n  FocusedDisplayId: 0\n"
                + "Input Dispatcher State at time of last ANR:\n  FocusedDisplayId: 12\n", "0\n");
        matches(new Main.FocusDisplayFilter(), "  FocusedDisplayId: 0\n"
                + "Input Dispatcher State at time of last ANR:\n  FocusedDisplayId: 12\n", "");
        matches(new Main.FocusDisplayFilter(), "Input Dispatcher State:\n  FocusedDisplayId: 0\n"
                + "  FocusedDisplayId: 12\n", "0\n12\n");
        rejectsFilter(bytes("Input Dispatcher State:\n  FocusedDisplayId: -1\n"), new Main.FocusDisplayFilter());
        rejectsFilter(bytes("Input Dispatcher State:\n  FocusedDisplayId: 0\n  FocusedDisplayId: 0 trailing\n"), new Main.FocusDisplayFilter());
        rejectsFilter(bytes("Input Dispatcher State:\n  FocusedDisplayId: 0\nInput Dispatcher State:\n"), new Main.FocusDisplayFilter());
        matches(new Main.ImeFilter(), "  mCurrentImeUserId=0\n  cachedEditorInfo:\n        inputType=secret\n"
                + "  mUserDataRepository:\n    userId=0\n      curClient=ClientState{test mSelfReportedDisplayId=0}\n"
                + "      curEditorInfo:\n        inputType=0x81 imeOptions=0x12000006\n        packageName=com.android.systemui autofillId=null\n"
                + "        surroundingText=private\n      cachedEditorInfo:\n        inputType=0x12\n"
                + "      history=private\n  Input method history:\n      curEditorInfo:\n        packageName=private\n",
                "  mCurrentImeUserId=0\n  mUserDataRepository:\n    userId=0\n"
                + "      curClient=ClientState{test mSelfReportedDisplayId=0}\n      curEditorInfo:\n"
                + "        inputType=0x81 imeOptions=0x12000006\n        packageName=com.android.systemui autofillId=null\n"
                + "      endEditor\n        inputType=0x12\n      endEditor\n");
        // The native parser excludes fields after endEditor. Preserve boundaries,
        // duplicate sections/users/fields, and malformed values for its rejection.
        matches(new Main.ImeFilter(), "  mUserDataRepository:\n    userId=0\n      curEditorInfo:\n"
                + "      curEditorInfo:\n        inputType=0x81\n  mCurrentImeUserId=10\n",
                "  mUserDataRepository:\n    userId=0\n      curEditorInfo:\n      curEditorInfo:\n"
                + "        inputType=0x81\n  mCurrentImeUserId=10\n  mCurrentImeUserId=10\n");
        // A repeated start header also matches sed's range-end expression.
        // Exclude following editor metadata until another range starts.
        matches(new Main.ImeFilter(), "  mUserDataRepository:\n  mUserDataRepository:\n"
                + "    userId=0\n      curEditorInfo:\n        inputType=0x81\n        packageName=private\n"
                + "  mCurrentImeUserId=0\n  mUserDataRepository:\n    userId=10\n      curEditorInfo:\n"
                + "        inputType=0x12\n  Input method history:\n        packageName=private\n",
                "  mUserDataRepository:\n  mUserDataRepository:\n  mCurrentImeUserId=0\n"
                + "  mUserDataRepository:\n    userId=10\n      curEditorInfo:\n        inputType=0x12\n");
        matches(new Main.MethodFilter(), "private\nUser State:\n  User 0\n    CredentialType: PASSWORD\n"
                + "    IsLockScreenDisabled: false\n    private=secret\n  User 10\n    CredentialType: PIN\n"
                + "Keys in namespace:\n  User 0\n    CredentialType: NONE\nUser State:\n  User 0\n    CredentialType: PASSWORD\n",
                "User State:\n  User 0\n    CredentialType: PASSWORD\n    IsLockScreenDisabled: false\n"
                + "  User 10\n    CredentialType: PIN\nUser State:\n  User 0\n    CredentialType: PASSWORD\n");
        matches(new Main.PowerFilter(), "  mWakefulness=Awake\n  mWakefulness=1\n    mWakefulness=Asleep\n  mWakefulness=Awake trailing\n",
                "  mWakefulness=Awake\n");
        matches(new Main.PanelFilter(), "Display 123 (HWC display 0):\n  powerMode=Off\n"
                + "  Virtual Display 456:\n    powerMode=On\n  private=secret\nDisplay unknown\n",
                "Display 123 (HWC display 0):\n  powerMode=Off\n  Virtual Display 456:\n    powerMode=On\n");
        matches(new Main.PowerFilter(), "  mWakefulness=Awake", "  mWakefulness=Awake\n");

        require("SNAPSHOT abc_123-Z".equals(Main.readRequest(bytes("SNAPSHOT abc_123-Z\n"))), "Valid request");
        require(Main.readRequest(bytes("")) == null, "EOF");
        rejectsRequest("SNAPSHOT abc\r\n");
        rejectsRequest("SNAPSHOT abc");
        rejectsRequest(repeat("x", Main.MAX_REQUEST + 1) + "\n");
        rejectsRequest("SNAPSHOT \u0000\n");
        rejectsFilter(bytes(repeat("x", Main.MAX_LINE + 1)), new Main.PowerFilter());
        rejectsFilter(bytes("private\r\n"), new Main.PowerFilter());
        rejectsFilter(new ByteArrayInputStream(new byte[] {(byte) 0xc3, (byte) 0x28}), new Main.PowerFilter());
        rejectsFilter(new InputStream() {
            private int count;

            @Override
            public int read() {
                return count++ <= Main.MAX_DUMP_BYTES ? '\n' : -1;
            }
        }, new Main.PowerFilter());
        rejectsFilter(bytes(repeat("  mWakefulness=Awake\n", 4000)), new Main.PowerFilter());
        protocol("SNAPSHOT synthetic_nonce\n", "ERROR synthetic_nonce unavailable\n");
        protocol("SNAPSHOT invalid nonce\n", "");
        protocol("SNAPSHOT synthetic_nonce\r\n", "");
        rawDescriptorProtocol();
        System.out.println("unlock-guard synthetic checks passed: " + checks);
    }

    private static ByteArrayInputStream bytes(String value) {
        return new ByteArrayInputStream(value.getBytes(StandardCharsets.UTF_8));
    }

    private static String repeat(String value, int count) {
        StringBuilder result = new StringBuilder();
        for (int i = 0; i < count; ++i) {
            result.append(value);
        }
        return result.toString();
    }

    private static void matches(Main.Filter filter, String input, String expected) throws IOException {
        StringBuilder output = new StringBuilder();
        Main.filter(bytes(input), output, filter);
        require(expected.equals(output.toString()), "Filtered schema mismatch");
        require(!output.toString().contains("secret") && !output.toString().contains("private"), "Private diagnostics escaped");
    }

    private static void rejectsRequest(String input) throws IOException {
        try {
            Main.readRequest(bytes(input));
            throw new AssertionError("Accepted invalid request");
        } catch (IOException expected) {
            checks++;
        }
    }

    private static void rejectsFilter(InputStream input, Main.Filter filter) throws IOException {
        try {
            Main.filter(input, new StringBuilder(), filter);
            throw new AssertionError("Accepted invalid/oversized diagnostic");
        } catch (IOException expected) {
            checks++;
        }
    }

    private static void require(boolean value, String message) {
        if (!value) {
            throw new AssertionError(message);
        }
        checks++;
    }

    private static void protocol(String input, String expected) throws IOException {
        InputStream previousInput = System.in;
        PrintStream previousOutput = System.out;
        PrintStream previousError = System.err;
        ByteArrayOutputStream output = new ByteArrayOutputStream();
        try {
            System.setIn(bytes(input));
            System.setOut(new PrintStream(output, true, "UTF-8"));
            Main.run(new String[0], bytes(input), new PrintStream(output, true, "UTF-8"));
            require(expected.equals(output.toString("UTF-8")), "Unexpected protocol or diagnostic output");
        } finally {
            System.setIn(previousInput);
            System.setOut(previousOutput);
            System.setErr(previousError);
        }
    }

    private static void rawDescriptorProtocol() throws Exception {
        String executable = System.getProperty("os.name").startsWith("Windows") ? "java.exe" : "java";
        Process process = new ProcessBuilder(Paths.get(System.getProperty("java.home"), "bin", executable).toString(),
                "-cp", System.getProperty("java.class.path"), MainTest.class.getName(), "redirected-system-streams").start();
        try {
            process.getOutputStream().write("SNAPSHOT raw_fd_probe\n".getBytes(StandardCharsets.US_ASCII));
            process.getOutputStream().close();
            require(process.waitFor(5, TimeUnit.SECONDS), "Raw descriptor probe timed out");
            require(process.exitValue() == 0, "Raw descriptor probe failed");
            String output = new String(readBounded(process.getInputStream()), StandardCharsets.UTF_8);
            require(output.equals("ERROR raw_fd_probe unavailable\n"), "Protocol used redirected System streams");
            require(readBounded(process.getErrorStream()).length == 0, "Protocol diagnostics escaped to stderr");
        } finally {
            process.destroyForcibly();
        }
    }

    private static byte[] readBounded(InputStream input) throws IOException {
        ByteArrayOutputStream output = new ByteArrayOutputStream();
        int next;
        while ((next = input.read()) != -1) {
            if (output.size() == 1024) {
                throw new IOException("Oversized synthetic protocol response");
            }
            output.write(next);
        }
        return output.toByteArray();
    }
}
