package sample

import (
	helper "example/helpers"
	. "strings"
)

type Greeter interface {
	Greet(name string) string
}

type UserService struct{}

func (s *UserService) Greet(name string) string {
	return FormatName(name)
}

func FormatName(name string) string {
	return TrimSpace(helper.Normalize(name))
}

const Version = "v1"

var defaultService = UserService{}

type Transformer = func(string) string
