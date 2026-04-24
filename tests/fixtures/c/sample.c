#include "shared.h"
#include <stdio.h>

typedef unsigned long size_type;

struct user {
    int age;
};

enum status {
    STATUS_OK,
    STATUS_ERR,
};

static int helper(void) {
    return 1;
}

int run(void) {
    printf("hi");
    return helper();
}
