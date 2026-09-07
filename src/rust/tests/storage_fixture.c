/* Bidirectional persistence fixture, compiled independently of Cargo's archive. */
#include "dtlv_storage.h"
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

static void check(int rc) {
  if (rc != MDB_SUCCESS) {
    fprintf(stderr, "DLMDB fixture: %s (%d)\n", mdb_strerror(rc), rc);
    exit(1);
  }
}

static void expect(int condition) {
  if (!condition) {
    fprintf(stderr, "DLMDB fixture content mismatch\n");
    exit(1);
  }
}

static void text_equal(MDB_val value, const char *expected) {
  expect(value.mv_size == strlen(expected));
  expect(memcmp(value.mv_data, expected, value.mv_size) == 0);
}

int main(int argc, char **argv) {
  if (argc != 3 || (strcmp(argv[1], "write") && strcmp(argv[1], "read"))) {
    fprintf(stderr, "usage: storage_fixture write|read DIRECTORY\n");
    return 2;
  }
  int writing = strcmp(argv[1], "write") == 0;
  MDB_env *env;
  MDB_txn *txn;
  MDB_dbi plain, duplicates;
  check(mdb_env_create(&env));
  check(mdb_env_set_maxdbs(env, 4));
  check(mdb_env_set_mapsize(env, 64 * 1024 * 1024));
  check(mdb_env_open(env, argv[2], MDB_NOTLS, 0600));
  check(mdb_txn_begin(env, NULL, writing ? 0 : MDB_RDONLY, &txn));
  unsigned int flags = writing ? MDB_CREATE | MDB_COUNTED | MDB_PREFIX_COMPRESSION : 0;
  check(mdb_dbi_open(txn, "plain", flags, &plain));
  check(mdb_dbi_open(txn, "duplicates", writing ? flags | MDB_DUPSORT | MDB_DUPFIXED : 0,
                     &duplicates));
  if (writing) {
    for (int i = 0; i < 128; ++i) {
      char key_buffer[64], value_buffer[16];
      snprintf(key_buffer, sizeof(key_buffer), "shared-prefix-%04d", i);
      snprintf(value_buffer, sizeof(value_buffer), "value-%04d", i);
      MDB_val key = {strlen(key_buffer), key_buffer};
      MDB_val value = {strlen(value_buffer), value_buffer};
      check(mdb_put(txn, plain, &key, &value, 0));
      for (int j = 0; j < 3; ++j) {
        snprintf(value_buffer, sizeof(value_buffer), "dup-%04d", j);
        value.mv_size = strlen(value_buffer);
        check(mdb_put(txn, duplicates, &key, &value, 0));
      }
    }
  } else {
    MDB_cursor *cursor;
    MDB_val key = {0, NULL}, value = {0, NULL};
    dtlv_key_iter *iter;
    check(mdb_cursor_open(txn, plain, &cursor));
    check(dtlv_key_iter_create(&iter, cursor, &key, &value,
                                DTLV_TRUE, DTLV_TRUE, DTLV_TRUE, NULL, NULL));
    /* Holders, cursor and transaction remain alive and stationary for the iterator. */
    for (int i = 0; i < 128; ++i) {
      char expected[64];
      expect(dtlv_key_iter_has_next(iter) == DTLV_TRUE);
      snprintf(expected, sizeof(expected), "shared-prefix-%04d", i);
      text_equal(key, expected);
      snprintf(expected, sizeof(expected), "value-%04d", i);
      text_equal(value, expected);
    }
    expect(dtlv_key_iter_has_next(iter) == DTLV_FALSE);
    dtlv_key_iter_destroy(iter);
    mdb_cursor_close(cursor);
    check(mdb_cursor_open(txn, duplicates, &cursor));
    for (int i = 0; i < 128; ++i) {
      char expected[64];
      for (int j = 0; j < 3; ++j) {
        check(mdb_cursor_get(cursor, &key, &value, MDB_NEXT));
        snprintf(expected, sizeof(expected), "shared-prefix-%04d", i);
        text_equal(key, expected);
        snprintf(expected, sizeof(expected), "dup-%04d", j);
        text_equal(value, expected);
        mdb_size_t count;
        check(mdb_cursor_count(cursor, &count));
        expect(count == 3);
      }
    }
    expect(mdb_cursor_get(cursor, &key, &value, MDB_NEXT) == MDB_NOTFOUND);
    mdb_cursor_close(cursor);
  }
  check(mdb_txn_commit(txn));
  mdb_env_close(env);
  return 0;
}
