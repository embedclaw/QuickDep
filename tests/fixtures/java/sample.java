package com.example.sample;

import java.util.List;
import static java.lang.Math.max;

interface Greeter {
    String greet(String name);
}

class UserService implements Greeter {
    public String greet(String name) {
        return format(name);
    }

    public int clamp(int value) {
        return max(value, 1);
    }

    private String format(String name) {
        return name.trim();
    }
}
