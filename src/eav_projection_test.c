#include "dtlv_storage.h"
#include <assert.h>
#include <errno.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <unistd.h>

#define CHECK(call) do { int rc_ = (call); if (rc_) { \
  fprintf(stderr, "%s: %s\n", #call, mdb_strerror(rc_)); abort(); } } while (0)

static void be32(unsigned char *p, uint32_t n) {
  for (int i = 3; i >= 0; i--, n >>= 8) p[i] = n;
}

static void be64(unsigned char *p, uint64_t n) {
  for (int i = 7; i >= 0; i--, n >>= 8) p[i] = n;
}

static void put(MDB_txn *txn, MDB_dbi dbi, int entity, int aid, int variant) {
  unsigned char k[8], v[96];
  be64(k, entity);
  memset(v, 'x', sizeof(v));
  be32(v, aid);
  v[4] = variant;
  MDB_val key = {sizeof(k), k}, value = {sizeof(v), v};
  /* Variable widths exercise ordinary duplicate leaves rather than DUPFIXED. */
  value.mv_size -= aid % 7;
  CHECK(mdb_put(txn, dbi, &key, &value, 0));
}

static int compare(MDB_txn *txn, MDB_dbi dbi, const int32_t *aids, int n, int selective) {
  MDB_cursor *project, *reference;
  CHECK(mdb_cursor_open(txn, dbi, &project));
  CHECK(mdb_cursor_open(txn, dbi, &reference));
  MDB_val key, refkey, firstval, values[8];
  dtlv_eav_projector *iter = NULL;
  if (selective) CHECK(dtlv_eav_projector_create(&iter, project, &key, values, aids, n));
  int entities = 0, complete = 0;
  for (int first = 1;; first = 0) {
    int actual = selective ? dtlv_eav_projector_next(iter)
        : dtlv_eav_project_next(project, &key, values, aids, n, first);
    int rc = mdb_cursor_get(reference, &refkey, &firstval,
                            first ? MDB_FIRST : MDB_NEXT_NODUP);
    if (rc == MDB_NOTFOUND) {
      assert(actual == MDB_NOTFOUND);
      break;
    }
    CHECK(rc);
    assert(key.mv_size == refkey.mv_size);
    assert(!memcmp(key.mv_data, refkey.mv_data, key.mv_size));
    int expected = DTLV_TRUE;
    for (int i = 0; i < n; i++) {
      unsigned char prefix[4];
      be32(prefix, aids[i]);
      MDB_val expectedval = {4, prefix};
      rc = mdb_cursor_get(reference, &refkey, &expectedval, MDB_GET_BOTH_RANGE);
      if (rc == MDB_NOTFOUND || memcmp(prefix, expectedval.mv_data, 4)) {
        expected = DTLV_FALSE;
        break;
      }
      CHECK(rc);
      if (actual == DTLV_TRUE) {
        assert(values[i].mv_size == expectedval.mv_size);
        assert(!memcmp(values[i].mv_data, expectedval.mv_data, values[i].mv_size));
      }
    }
    assert(actual == expected);
    if (actual == DTLV_TRUE) complete++;
    entities++;
  }
  assert((selective ? dtlv_eav_projector_next(iter)
          : dtlv_eav_project_next(project, &key, values, aids, n, 0)) == MDB_NOTFOUND);
  dtlv_eav_projector_destroy(iter);
  mdb_cursor_close(reference);
  mdb_cursor_close(project);
  printf("projection %d fields, selective=%d: %d entities, %d complete\n", n, selective, entities, complete);
  return entities;
}

int main(void) {
  char directory[] = "/tmp/dtlv-eav-projection-XXXXXX";
  assert(mkdtemp(directory));
  for (int compressed = 0; compressed <= 1; compressed++) {
    MDB_env *env;
    MDB_txn *txn;
    MDB_dbi dbi;
    CHECK(mdb_env_create(&env));
    CHECK(mdb_env_set_mapsize(env, 32 * 1024 * 1024));
    CHECK(mdb_env_open(env, directory, MDB_NOTLS, 0600));
    CHECK(mdb_txn_begin(env, NULL, 0, &txn));
    CHECK(mdb_dbi_open(txn, NULL, MDB_DUPSORT |
                        (compressed ? MDB_PREFIX_COMPRESSION : 0), &dbi));
    const int32_t fields[] = {2, 7, 9};
    assert(compare(txn, dbi, fields, 3, 0) == 0);
    for (int e = 1; e <= 1000; e++) {
      if (e % 11 == 0) {
        put(txn, dbi, e, 7, 0); /* Single-datom entities. */
      } else {
        for (int a = 1; a <= 12; a++) {
          if (a == 7 && e % 3 == 0) continue;
          put(txn, dbi, e, a, 0);
          if (a == 7) put(txn, dbi, e, a, 1); /* First duplicate wins. */
        }
      }
    }
    /* Multi-page duplicate tree with selected fields far apart. */
    for (int a = 1; a <= 3000; a++) put(txn, dbi, 5000, a, 0);
    CHECK(mdb_txn_commit(txn));
    CHECK(mdb_txn_begin(env, NULL, MDB_RDONLY, &txn));
    for (int mode = 0; mode <= 1; mode++)
      assert(compare(txn, dbi, fields, 3, mode) == 1001);
    const int32_t one[] = {7};
    for (int mode = 0; mode <= 1; mode++)
      assert(compare(txn, dbi, one, 1, mode) == 1001);
    const int32_t sparse[] = {1, 255, 256, 1000, 3000};
    for (int mode = 0; mode <= 1; mode++)
      assert(compare(txn, dbi, sparse, 5, mode) == 1001);
    const int32_t missing_first[] = {0, 7}, missing_last[] = {7, 4000};
    for (int mode = 0; mode <= 1; mode++)
      assert(compare(txn, dbi, missing_first, 2, mode) == 1001);
    for (int mode = 0; mode <= 1; mode++)
      assert(compare(txn, dbi, missing_last, 2, mode) == 1001);
    MDB_cursor *cursor;
    CHECK(mdb_cursor_open(txn, dbi, &cursor));
    MDB_val key, values[3];
    const int32_t invalid[] = {7, 2};
    assert(dtlv_eav_project_next(cursor, &key, values, invalid, 2, 1) == EINVAL);
    assert(dtlv_eav_project_next(cursor, &key, values, fields, 0, 1) == EINVAL);
    assert(dtlv_eav_project_next(NULL, &key, values, fields, 3, 1) == EINVAL);
    mdb_cursor_close(cursor);
    mdb_txn_abort(txn);
    mdb_env_close(env);
    char path[256];
    snprintf(path, sizeof(path), "%s/data.mdb", directory); assert(!unlink(path));
    snprintf(path, sizeof(path), "%s/lock.mdb", directory); assert(!unlink(path));
  }
  assert(!rmdir(directory));
  puts("EAV projection tests passed (plain and prefix-compressed pages)");
  return 0;
}
