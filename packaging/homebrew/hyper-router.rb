require "securerandom"

class HyperRouter < Formula
  desc "Local proxy for coding assistant APIs"
  homepage "https://github.com/alpoxdev/hyper-router"
  license "MIT"

  on_macos do
    on_arm do
      url "https://github.com/alpoxdev/hyper-router/releases/download/v@VERSION@/hyperrouter-@VERSION@-aarch64-apple-darwin.tar.gz"
      sha256 "@SHA256_AARCH64_APPLE_DARWIN@"
    end
    on_intel do
      url "https://github.com/alpoxdev/hyper-router/releases/download/v@VERSION@/hyperrouter-@VERSION@-x86_64-apple-darwin.tar.gz"
      sha256 "@SHA256_X86_64_APPLE_DARWIN@"
    end
  end

  on_linux do
    on_arm do
      url "https://github.com/alpoxdev/hyper-router/releases/download/v@VERSION@/hyperrouter-@VERSION@-aarch64-unknown-linux-gnu.tar.gz"
      sha256 "@SHA256_AARCH64_UNKNOWN_LINUX_GNU@"
    end
    on_intel do
      url "https://github.com/alpoxdev/hyper-router/releases/download/v@VERSION@/hyperrouter-@VERSION@-x86_64-unknown-linux-gnu.tar.gz"
      sha256 "@SHA256_X86_64_UNKNOWN_LINUX_GNU@"
    end
  end

  def install
    bin.install "hyperrouter"
  end

  # ponytail: support older Homebrew; use post_install_steps when that compatibility is no longer needed.
  def post_install
    config_dir = etc/"hyper-router"
    config = config_dir/"config.yaml"
    return if config.exist?

    config_dir.mkpath
    config_dir.chmod 0700
    (config_dir/"auth").mkpath
    keys_file = config_dir/"keys.env"
    unless keys_file.exist?
      keys_file.write <<~EOS
        HYPERROUTER_CLIENT_KEY=#{SecureRandom.hex(24)}
      EOS
      keys_file.chmod 0600
    end
    keys = keys_file.read.lines.reject { |line| line.strip.empty? || line.lstrip.start_with?("#") }
                    .to_h { |line| line.strip.split("=", 2) }
    client_key = keys.fetch("HYPERROUTER_CLIENT_KEY")
    unless client_key.match?(/\A[0-9a-f]{48}\z/)
      odie "Invalid keys in #{keys_file}; restore the keys or write config.yaml yourself."
    end
    config.write <<~EOS
      server:
        host: "127.0.0.1"
        port: 8317
      access:
        api-keys:
          - "#{client_key}"
      oauth:
        auth-dir: "#{config_dir}/auth"
    EOS
    config.chmod 0600
  end

  def caveats
    <<~EOS
      Config and sign-in credentials are kept in #{etc}/hyper-router.
      The client key is saved in #{etc}/hyper-router/keys.env.
      The dashboard needs no password; set ADMIN_PASSWORD to require one.
      The default server listens only on 127.0.0.1:8317.
      Start it with brew services start hyper-router, then open:
        http://127.0.0.1:8317/management.html
      If port 8317 is already in use, change server.port in config.yaml first.
    EOS
  end

  service do
    run [opt_bin/"hyperrouter", "--config", etc/"hyper-router/config.yaml"]
    keep_alive true
    log_path var/"log/hyper-router.log"
    error_log_path var/"log/hyper-router.log"
  end

  test do
    assert_match "hyperrouter #{version}", shell_output("#{bin}/hyperrouter --version")
  end
end
