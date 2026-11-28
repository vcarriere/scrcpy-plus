package com.scrcpyplus.unlockguard;

import java.io.BufferedInputStream;
import java.io.IOException;
import java.io.InputStream;

/** Strict bounded root-wire decoder for PowerManagerServiceDumpProto.wakefulness. */
final class PowerProto {

    static final int ABSENT = -1;
    private static final int MAX_BYTES = 4 * 1024 * 1024;
    private static final int MAX_GROUP_DEPTH = 16;

    private final InputStream input;
    private final byte[] discard = new byte[8192];
    private int count;
    private boolean seen;
    private int wakefulness = ABSENT;

    private PowerProto(InputStream input) {
        this.input = new BufferedInputStream(input, 8192);
    }

    static int read(InputStream input) throws IOException {
        PowerProto decoder = new PowerProto(input);
        decoder.fields(0, 0);
        return decoder.wakefulness;
    }

    private void fields(int group, int depth) throws IOException {
        while (true) {
            int first = next();
            if (first == -1) {
                if (group != 0) {
                    throw new IOException("Truncated group");
                }
                return;
            }
            long tag = varint(first);
            if (tag <= 0 || tag > 0xffffffffL) {
                throw new IOException("Invalid tag");
            }
            int field = (int) (tag >>> 3);
            int wire = (int) (tag & 7);
            if (field == 0) {
                throw new IOException("Invalid field");
            }
            if (wire == 4) {
                if (group != field) {
                    throw new IOException("Invalid group end");
                }
                return;
            }
            if (depth == 0 && field == 3) {
                if (seen || wire != 0) {
                    throw new IOException("Ambiguous root wakefulness");
                }
                long value = varint(required());
                if (value < 0 || value > 3) {
                    throw new IOException("Unknown wakefulness");
                }
                seen = true;
                wakefulness = (int) value;
                continue;
            }
            switch (wire) {
                case 0:
                    varint(required());
                    break;
                case 1:
                    skip(8);
                    break;
                case 2:
                    long length = varint(required());
                    if (length < 0 || length > MAX_BYTES) {
                        throw new IOException("Invalid length");
                    }
                    // Length-delimited fields can be strings/bytes/messages.
                    // Their payload stays opaque; only root field3 is state.
                    skip((int) length);
                    break;
                case 3:
                    if (depth == MAX_GROUP_DEPTH) {
                        throw new IOException("Excessive group nesting");
                    }
                    fields(field, depth + 1);
                    break;
                case 5:
                    skip(4);
                    break;
                default:
                    throw new IOException("Invalid wire type");
            }
        }
    }

    private int next() throws IOException {
        int value = input.read();
        if (value != -1 && ++count > MAX_BYTES) {
            throw new IOException("Oversized proto");
        }
        return value;
    }

    private int required() throws IOException {
        int value = next();
        if (value == -1) {
            throw new IOException("Truncated proto");
        }
        return value;
    }

    private long varint(int first) throws IOException {
        long value = first & 127;
        if ((first & 128) == 0) {
            return value;
        }
        for (int index = 1; index < 10; ++index) {
            int next = required();
            if (index == 9 && (next & 254) != 0) {
                throw new IOException("Overflowing varint");
            }
            value |= (long) (next & 127) << (7 * index);
            if ((next & 128) == 0) {
                if (next == 0) {
                    throw new IOException("Noncanonical varint");
                }
                return value;
            }
        }
        throw new IOException("Overflowing varint");
    }

    private void skip(int length) throws IOException {
        while (length != 0) {
            int read = input.read(discard, 0, Math.min(length, discard.length));
            if (read <= 0) {
                throw new IOException("Truncated field");
            }
            count += read;
            if (count > MAX_BYTES) {
                throw new IOException("Oversized proto");
            }
            length -= read;
        }
    }
}
