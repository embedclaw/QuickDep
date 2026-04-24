#!/usr/bin/env python3

from __future__ import annotations

import argparse
from pathlib import Path


FORMULA_TEMPLATE = """class Quickdep < Formula
  desc "Rust MCP service for project dependency analysis"
  homepage "https://github.com/{repository}"
  version "{version}"
  license "MIT"

  on_macos do
    if Hardware::CPU.arm?
      url "https://github.com/{repository}/releases/download/v{version}/quickdep-darwin-aarch64.tar.gz"
      sha256 "{darwin_aarch64_sha}"
    else
      url "https://github.com/{repository}/releases/download/v{version}/quickdep-darwin-x86_64.tar.gz"
      sha256 "{darwin_x86_64_sha}"
    end
  end

  on_linux do
    if Hardware::CPU.arm?
      url "https://github.com/{repository}/releases/download/v{version}/quickdep-linux-aarch64.tar.gz"
      sha256 "{linux_aarch64_sha}"
    else
      url "https://github.com/{repository}/releases/download/v{version}/quickdep-linux-x86_64.tar.gz"
      sha256 "{linux_x86_64_sha}"
    end
  end

  def install
    bin.install "quickdep"
  end

  test do
    assert_match version.to_s, shell_output("#{{bin}}/quickdep --version")
  end
end
"""


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser()
    parser.add_argument("--version", required=True)
    parser.add_argument("--repository", required=True)
    parser.add_argument("--tap-repo", required=True)
    parser.add_argument("--darwin-aarch64-sha", required=True)
    parser.add_argument("--darwin-x86_64-sha", required=True)
    parser.add_argument("--linux-aarch64-sha", required=True)
    parser.add_argument("--linux-x86_64-sha", required=True)
    return parser.parse_args()


def main() -> None:
    args = parse_args()
    tap_repo = Path(args.tap_repo)
    formula_dir = tap_repo / "Formula"
    formula_dir.mkdir(parents=True, exist_ok=True)
    formula_path = formula_dir / "quickdep.rb"
    formula_path.write_text(
        FORMULA_TEMPLATE.format(
            version=args.version,
            repository=args.repository,
            darwin_aarch64_sha=args.darwin_aarch64_sha,
            darwin_x86_64_sha=args.darwin_x86_64_sha,
            linux_aarch64_sha=args.linux_aarch64_sha,
            linux_x86_64_sha=args.linux_x86_64_sha,
        )
    )


if __name__ == "__main__":
    main()
