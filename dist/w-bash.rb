class WBash < Formula
  desc "Workspace tool for AI agents with compact, low-token output"
  homepage "https://github.com/KlyneChrysler/wbash"
  version "0.1.0"
  license "MIT"

  on_macos do
    if Hardware::CPU.arm?
      url "https://github.com/KlyneChrysler/wbash/releases/download/v#{version}/w-bash-v#{version}-aarch64-apple-darwin.tar.gz"
      sha256 "REPLACE_WITH_SHA256"
    else
      url "https://github.com/KlyneChrysler/wbash/releases/download/v#{version}/w-bash-v#{version}-x86_64-apple-darwin.tar.gz"
      sha256 "REPLACE_WITH_SHA256"
    end
  end

  on_linux do
    if Hardware::CPU.arm?
      url "https://github.com/KlyneChrysler/wbash/releases/download/v#{version}/w-bash-v#{version}-aarch64-unknown-linux-gnu.tar.gz"
      sha256 "REPLACE_WITH_SHA256"
    else
      url "https://github.com/KlyneChrysler/wbash/releases/download/v#{version}/w-bash-v#{version}-x86_64-unknown-linux-gnu.tar.gz"
      sha256 "REPLACE_WITH_SHA256"
    end
  end

  def install
    bin.install "w"
    bin.install "w-bash"
  end

  test do
    assert_match version.to_s, shell_output("#{bin}/w --version")
  end
end
