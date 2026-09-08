#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include "usearch/c/usearch.h"

int main(int argc, char **argv) {
  if (argc != 3) return 2;
  usearch_error_t error = NULL;
  usearch_init_options_t options = {0};
  options.metric_kind = usearch_metric_l2sq_k;
  options.quantization = usearch_scalar_f32_k;
  options.dimensions = 3;
  usearch_index_t index = usearch_init(&options, &error);
  if (error || !index) return 1;
  float vector[3] = {1, 2, 3};
  if (!strcmp(argv[1], "write")) {
    usearch_reserve(index, 4, &error);
    if (!error) usearch_add(index, 42, vector, usearch_scalar_f32_k, &error);
    if (!error) usearch_save(index, argv[2], &error);
  } else {
    FILE *file = fopen(argv[2], "rb");
    if (!file) return 1;
    if (fseek(file, 0, SEEK_END)) return 1;
    long length = ftell(file);
    if (length <= 0 || fseek(file, 0, SEEK_SET)) return 1;
    void *bytes = malloc((size_t)length);
    if (!bytes || fread(bytes, 1, (size_t)length, file) != (size_t)length) return 1;
    fclose(file);
    usearch_load_buffer(index, bytes, (size_t)length, &error);
    free(bytes);
    usearch_key_t key = 0;
    usearch_distance_t distance = -1;
    size_t found = error ? 0 : usearch_search(index, vector, usearch_scalar_f32_k, 1, &key, &distance, &error);
    if (found != 1 || key != 42 || distance != 0) error = "C/Rust vector fixture mismatch";
  }
  if (error) fprintf(stderr, "%s\n", error);
  int failed = error != NULL;
  usearch_free(index, &error);
  return failed;
}
