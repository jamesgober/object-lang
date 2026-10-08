/* Source of the clang-built fixtures sample-x86_64.o and sample-aarch64.o.
 *
 *   clang --target=x86_64-unknown-linux-gnu  -O1 -c -fno-asynchronous-unwind-tables \
 *         -fno-unwind-tables -fno-pic -fno-ident sample.c -o sample-x86_64.o
 *   clang --target=aarch64-unknown-linux-gnu -O1 -c -fno-asynchronous-unwind-tables \
 *         -fno-unwind-tables -fno-pic -fno-ident sample.c -o sample-aarch64.o
 *
 * (clang 18.1.8). The functions exercise calls, data and string references, a
 * pointer table, zero-initialized storage, mergeable strings, and a weak symbol.
 * (No thread-local storage: TLS relocations are not modelled before v0.5.)
 */
extern int puts(const char *s);
extern int shared_counter;

static int hits;                 /* .bss, local */
int table_size = 3;              /* .data */
const char *names[] = { "alpha", "beta", "gamma" }; /* .data, with relocations */

__attribute__((weak)) int hook(int x) { return x + 1; }

static int helper(int x) { hits++; return hook(x) * 2; }

int entry(int x) {
    shared_counter += x;
    puts(names[x % table_size]);
    return helper(x);
}
