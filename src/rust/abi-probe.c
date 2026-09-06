/* Host ABI observations for the Rust binding audit. Run from the repository root:
 * clang -std=c11 -Wall -Wextra -Werror -Isrc src/rust/abi-probe.c -o /tmp/dtlvnative-abi
 * /tmp/dtlvnative-abi
 * These measurements are target-specific, not portable Rust type definitions.
 */
#include <stddef.h>
#include <stdio.h>
#include "dtlv.h"
#define TYPE(T) printf("%s %zu %zu\n", #T, sizeof(T), _Alignof(T))
#define FIELD(T, F) printf("%s.%s %zu\n", #T, #F, offsetof(T, F))
int main(void) {
  TYPE(size_t); TYPE(mdb_size_t); TYPE(mdb_mode_t); TYPE(mdb_filehandle_t);
  TYPE(MDB_dbi); TYPE(MDB_cursor_op); TYPE(MDB_val); TYPE(MDB_stat); TYPE(MDB_envinfo);
  TYPE(usearch_key_t); TYPE(usearch_distance_t); TYPE(usearch_metric_kind_t);
  TYPE(usearch_scalar_kind_t); TYPE(usearch_init_options_t);
  FIELD(MDB_val, mv_size); FIELD(MDB_val, mv_data);
  FIELD(usearch_init_options_t, metric_kind); FIELD(usearch_init_options_t, metric);
  FIELD(usearch_init_options_t, quantization); FIELD(usearch_init_options_t, dimensions);
  FIELD(usearch_init_options_t, multi);
  printf("DTLV_TRUE %d\nDTLV_FALSE %d\n", DTLV_TRUE, DTLV_FALSE);
  return 0;
}
