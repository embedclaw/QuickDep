#include "shared.hpp"
#include <vector>

namespace app {

class Base {};

class UserService : public Base {
public:
    UserService() = default;
    ~UserService() = default;

    int run() {
        return helper();
    }
};

}

int helper() {
    return 1;
}

int app::UserService::build() {
    return helper();
}
