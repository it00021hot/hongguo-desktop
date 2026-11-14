#include <stdio.h>

int main(int argc, char **argv) {
  if (argc != 3)
    return 2;
#ifdef __APPLE__
  if (renamex_np(argv[1], argv[2], RENAME_SWAP) == 0)
    return 0;
  perror("native media atomic exchange");
  return 1;
#else
  return 3;
#endif
}
