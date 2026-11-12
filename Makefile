ARTIFACT ?= kicker
TARGET ?= $(CARGO_BUILD_TARGET)
RELEASE_ARGS ?= --apply
override RELEASE_ARGS := --group kicker $(RELEASE_ARGS)
include standards/make/common.mk
CRAFTGUILD_LAUNCH = sh tools/standards.sh

acceptance: build
