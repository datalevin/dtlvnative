package datalevin.dtlvnative;

import java.nio.file.Files;
import java.nio.file.Path;
import java.util.Comparator;
import java.util.concurrent.CompletableFuture;
import java.util.concurrent.TimeUnit;
import java.util.stream.Stream;
import org.bytedeco.javacpp.BytePointer;
import org.bytedeco.javacpp.IntPointer;

/** Storage-only JNI regression. No model download or vector fixture is needed. */
public final class PreparedCopyTest {
    private static void check(int rc) {
        if (rc != 0) throw new AssertionError("Native call failed: " + rc);
    }

    private static DTLV.MDB_env open(Path path, int flags) {
        DTLV.MDB_env env = new DTLV.MDB_env();
        check(DTLV.mdb_env_create(env));
        try {
            check(DTLV.mdb_env_set_mapsize(env, 16L << 20));
            check(DTLV.mdb_env_set_maxreaders(env, 4));
            check(DTLV.mdb_env_open(env, path.toString(), flags, 0600));
            return env;
        } catch (RuntimeException | Error e) {
            DTLV.mdb_env_close(env);
            throw e;
        }
    }

    private static void put(DTLV.MDB_env env, String value) {
        DTLV.MDB_txn txn = new DTLV.MDB_txn();
        check(DTLV.mdb_txn_begin(env, null, 0, txn));
        boolean committed = false;
        try (IntPointer dbi = new IntPointer(1);
             BytePointer keyBytes = new BytePointer("key");
             BytePointer valueBytes = new BytePointer(value);
             DTLV.MDB_val key = new DTLV.MDB_val();
             DTLV.MDB_val data = new DTLV.MDB_val()) {
            check(DTLV.mdb_dbi_open(txn, (String)null, 0, dbi));
            key.mv_data(keyBytes).mv_size(3);
            data.mv_data(valueBytes).mv_size(value.length());
            check(DTLV.mdb_put(txn, dbi.get(), key, data, 0));
            // Commit consumes the transaction even when it fails.
            committed = true;
            check(DTLV.mdb_txn_commit(txn));
        } finally {
            if (!committed) DTLV.mdb_txn_abort(txn);
        }
    }

    private static void expect(Path path, String expected) {
        DTLV.MDB_env env = open(path, DTLV.MDB_RDONLY);
        DTLV.MDB_txn txn = new DTLV.MDB_txn();
        try {
            check(DTLV.mdb_txn_begin(env, null, DTLV.MDB_RDONLY, txn));
            try (IntPointer dbi = new IntPointer(1);
                 BytePointer keyBytes = new BytePointer("key");
                 DTLV.MDB_val key = new DTLV.MDB_val();
                 DTLV.MDB_val data = new DTLV.MDB_val()) {
                check(DTLV.mdb_dbi_open(txn, (String)null, 0, dbi));
                key.mv_data(keyBytes).mv_size(3);
                check(DTLV.mdb_get(txn, dbi.get(), key, data));
                byte[] bytes = new byte[(int)data.mv_size()];
                new BytePointer(data.mv_data()).get(bytes);
                if (!expected.equals(new String(bytes, java.nio.charset.StandardCharsets.UTF_8)))
                    throw new AssertionError("Snapshot contains a later value");
            } finally {
                DTLV.mdb_txn_abort(txn);
            }
        } finally {
            DTLV.mdb_env_close(env);
        }
    }

    private static void offThread(Runnable action) throws Exception {
        try {
            CompletableFuture.runAsync(action).get(10, TimeUnit.SECONDS);
        } catch (java.util.concurrent.TimeoutException e) {
            // A blocked native call cannot be interrupted safely. Exit before
            // cleanup could close its environment while it is still running.
            System.err.println("Native copy/writer operation did not finish");
            System.exit(1);
        }
    }

    public static void main(String[] args) throws Exception {
        Path root = Files.createTempDirectory("dtlv-prepared-jni-");
        try {
            for (int flags : new int[] {0, DTLV.MDB_NOTLS}) {
                for (int mode : new int[] {0, DTLV.MDB_CP_COMPACT}) {
                    Path source = Files.createDirectory(root.resolve("source-" + flags + "-" + mode));
                    Path dest = Files.createDirectory(root.resolve("copy-" + flags + "-" + mode));
                    Path legacy = Files.createDirectory(root.resolve("legacy-" + flags + "-" + mode));
                    DTLV.MDB_env env = open(source, flags);
                    try {
                        put(env, "before");
                        DTLV.MDB_copy copy = new DTLV.MDB_copy();
                        check(DTLV.mdb_env_copy_prepare(env, dest.toString(), mode, copy));
                        if (copy.isNull()) throw new AssertionError("No prepared handle");
                        try {
                            offThread(() -> put(env, "after"));
                            if ((flags & DTLV.MDB_NOTLS) != 0)
                                offThread(() -> check(DTLV.mdb_env_copy_execute(copy)));
                            else check(DTLV.mdb_env_copy_execute(copy));
                            if (DTLV.mdb_env_copy_execute(copy) == 0)
                                throw new AssertionError("Prepared copy executed twice");
                        } finally {
                            check(DTLV.mdb_env_copy_close(copy));
                            copy.setNull();
                        }
                        expect(dest, "before");
                        check(DTLV.mdb_env_copy2(env, legacy.toString(), mode));
                        expect(legacy, "after");
                    } finally {
                        DTLV.mdb_env_close(env);
                    }
                }
            }
            System.out.println("Prepared copy JNI: both modes and reader policies passed");
        } finally {
            try (Stream<Path> paths = Files.walk(root)) {
                for (Path path : paths.sorted(Comparator.reverseOrder()).toList()) Files.delete(path);
            }
        }
    }
}
