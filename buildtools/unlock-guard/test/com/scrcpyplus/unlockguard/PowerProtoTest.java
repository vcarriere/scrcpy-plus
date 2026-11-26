package com.scrcpyplus.unlockguard;

import java.io.ByteArrayInputStream;
import java.io.ByteArrayOutputStream;
import java.io.IOException;
import java.io.InputStream;

public final class PowerProtoTest {
    private static int checks;

    private PowerProtoTest() {
        /* not instantiable */
    }

    public static void main(String[] args) throws Exception {
        for (int value = 0; value <= 3; ++value) {
            valid(bytes(0x18, value), value);
        }
        valid(bytes(), PowerProto.ABSENT);
        valid(bytes(0x08, 0), PowerProto.ABSENT);
        valid(bytes(0x08, 0x96, 0x01, 0x21, 0, 0, 0, 0, 0, 0, 0, 0,
                0x2a, 3, 0x18, 0xff, 0xff, 0x35, 0, 0, 0, 0, 0x18, 1), 1);
        // Nested-looking bytes and group fields are not root state.
        valid(bytes(0x42, 2, 0x18, 2, 0x18, 1), 1);
        valid(bytes(0x43, 0x18, 2, 0x44, 0x18, 1), 1);
        valid(bytes(0x08, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 1, 0x18, 1), 1);
        invalid(bytes(0x18, 1, 0x18, 1));
        invalid(bytes(0x1d, 0, 0, 0, 0));
        invalid(bytes(0x1a, 1, 1));
        invalid(bytes(0x18, 4));
        invalid(bytes(0x18));
        invalid(bytes(0x80));
        invalid(bytes(0x18, 0x80, 0));
        invalid(bytes(0x18, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 1));
        invalid(bytes(0x08, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 2));
        invalid(bytes(0));
        invalid(bytes(0x80, 0x80, 0x80, 0x80, 0x10));
        invalid(bytes(0x0e));
        invalid(bytes(0x0a, 2, 0));
        invalid(bytes(0x18, 1, 0x0a, 2, 0));
        invalid(bytes(0x0a, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 1));
        invalid(bytes(0x09, 0));
        invalid(bytes(0x0d, 0));
        invalid(bytes(0x0c));
        invalid(bytes(0x43));
        invalid(bytes(0x43, 0x4c));
        ByteArrayOutputStream deep = new ByteArrayOutputStream();
        for (int i = 0; i < 17; ++i) { deep.write(0x43); }
        for (int i = 0; i < 17; ++i) { deep.write(0x44); }
        invalid(new ByteArrayInputStream(deep.toByteArray()));
        invalid(new InputStream() {
            private int index;

            @Override
            public int read() {
                if (index == 4 * 1024 * 1024 + 2) { return -1; }
                return index++ % 2 == 0 ? 8 : 0;
            }
        });
        String[] names = {"Asleep", "Awake", "Dreaming", "Dozing"};
        for (int state = 0; state < names.length; ++state) {
            if (PowerDump.textWakefulness("@@power\n  mWakefulness=" + names[state] + "\n") != state) {
                throw new AssertionError("Incorrect text enum");
            }
            checks++;
        }
        invalidText("@@power\n");
        invalidText("@@power\n  mWakefulness=Awake\n  mWakefulness=Awake\n");
        invalidText("@@power\n  mWakefulness=Unknown\n");
        System.out.println("power-proto synthetic checks passed: " + checks);
    }

    private static ByteArrayInputStream bytes(int... values) {
        byte[] data = new byte[values.length];
        for (int i = 0; i < data.length; ++i) { data[i] = (byte) values[i]; }
        return new ByteArrayInputStream(data);
    }

    private static void valid(InputStream input, int expected) throws IOException {
        if (PowerProto.read(input) != expected) { throw new AssertionError("Incorrect root state"); }
        checks++;
    }

    private static void invalid(InputStream input) throws IOException {
        try {
            PowerProto.read(input);
            throw new AssertionError("Accepted malformed proto");
        } catch (IOException expected) {
            checks++;
        }
    }

    private static void invalidText(String value) throws IOException {
        try {
            PowerDump.textWakefulness(value);
            throw new AssertionError("Accepted ambiguous text state");
        } catch (IOException expected) {
            checks++;
        }
    }
}
