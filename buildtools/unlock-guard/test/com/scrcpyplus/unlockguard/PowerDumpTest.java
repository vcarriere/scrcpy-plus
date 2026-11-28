package com.scrcpyplus.unlockguard;

import java.io.ByteArrayInputStream;
import java.io.IOException;

public final class PowerDumpTest {
    private static int checks;

    private PowerDumpTest() {
        /* not instantiable */
    }

    public static void main(String[] args) throws Exception {
        String[] names = {"Asleep", "Awake", "Dreaming", "Dozing"};
        for (int state = 0; state < names.length; ++state) {
            require(PowerDump.current(proto(state), noText()).equals(text(names[state])), "Incorrect explicit enum mapping");
            final String expected = text(names[state]);
            final int[] calls = {0};
            String result = PowerDump.current(proto(PowerProto.ABSENT), new PowerDump.TextRead() {
                @Override
                public String read() {
                    calls[0]++;
                    return expected;
                }
            });
            require(result.equals(expected), "Absent enum did not use authoritative text");
            require(calls[0] == 1, "Absent enum must make exactly one fresh text query");
        }
        final int[] fresh = {0};
        PowerDump.TextRead changing = new PowerDump.TextRead() {
            @Override
            public String read() {
                return text(fresh[0]++ == 0 ? "Asleep" : "Awake");
            }
        };
        require(PowerDump.current(proto(-1), changing).equals(text("Asleep")), "First absent sample changed");
        require(PowerDump.current(proto(-1), changing).equals(text("Awake")) && fresh[0] == 2, "Power state was cached");
        rejects(proto(-2), noText());
        rejects(proto(4), noText());
        rejects(new PowerDump.ProtoRead() {
            @Override
            public int read() throws IOException { throw new IOException("Synthetic I/O failure"); }
        }, noText());
        for (byte[] malformed : new byte[][] {{24, 1, 24, 1}, {29, 0, 0, 0, 0}, {24, 4}, {24, 1, 10, 2, 0}}) {
            final byte[] data = malformed;
            rejects(new PowerDump.ProtoRead() {
                @Override
                public int read() throws IOException { return PowerProto.read(new ByteArrayInputStream(data)); }
            }, noText());
        }
        try {
            PowerDump.current(new PowerDump.ProtoRead() {
                @Override
                public int read() { throw new LinkageError("Synthetic unsupported runtime"); }
            }, noText());
            throw new AssertionError("Converted runtime failure into text fallback");
        } catch (LinkageError expected) {
            checks++;
        }
        for (String invalid : new String[] {"@@power\n", "@@power\n  mWakefulness=Unknown\n", text("Awake") + "  mWakefulness=Awake\n"}) {
            final String value = invalid;
            rejects(proto(-1), new PowerDump.TextRead() {
                @Override
                public String read() { return value; }
            });
        }
        rejects(proto(-1), new PowerDump.TextRead() {
            @Override
            public String read() throws IOException { throw new IOException("Synthetic text failure"); }
        });
        require(PowerDump.current(new PowerDump.ProtoRead() {
            @Override
            public int read() throws IOException { return PowerProto.read(new ByteArrayInputStream(new byte[0])); }
        }, new PowerDump.TextRead() {
            @Override
            public String read() { return text("Asleep"); }
        }).equals(text("Asleep")), "Valid absent proto must confirm text without inferring awake");
        System.out.println("power-dump synthetic checks passed: " + checks);
    }

    private static PowerDump.ProtoRead proto(final int state) {
        return new PowerDump.ProtoRead() {
            @Override
            public int read() { return state; }
        };
    }

    private static PowerDump.TextRead noText() {
        return new PowerDump.TextRead() {
            @Override
            public String read() { throw new AssertionError("Unexpected text fallback"); }
        };
    }

    private static String text(String state) {
        return "@@power\n  mWakefulness=" + state + "\n";
    }

    private static void rejects(PowerDump.ProtoRead proto, PowerDump.TextRead text) throws Exception {
        try {
            PowerDump.current(proto, text);
            throw new AssertionError("Accepted invalid Power read");
        } catch (IOException expected) {
            checks++;
        }
    }

    private static void require(boolean value, String message) {
        if (!value) { throw new AssertionError(message); }
        checks++;
    }
}
