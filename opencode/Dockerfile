# OpenCode 1 + oh-my-opencode + ocmonitor on top of ocli-base (Dockerfile.base)
# Built by ./bin/build-container-opencode
FROM ocli-base

# --no-modify-path: ~/bin/opencode symlink below puts it on PATH
RUN curl -fsSL https://opencode.ai/install | bash -s -- --no-modify-path
RUN git clone https://github.com/Shlomob/ocmonitor-share.git && cd ocmonitor-share && ./install.sh

## FAST UPGRADE PATH
RUN echo bump_008
RUN /home/iphands/.opencode/bin/opencode upgrade
RUN if [ "$(uname -m)" = x86_64 ]; then npm install -g oh-my-opencode-linux-x64; else npm install -g oh-my-opencode-linux-arm64; fi
RUN source /home/iphands/.bash_profile ; source /home/iphands/.bashrc ; ./.bun/bin/bunx oh-my-opencode install --no-tui --claude=no --gemini=no --copilot=no --opencode-zen=no --zai-coding-plan=no

RUN ln -s /home/iphands/ocmonitor-share/ocmonitor-wrapper.sh /home/iphands/bin/ocm && \
    ln -s /home/iphands/.opencode/bin/opencode /home/iphands/bin/opencode || true

CMD [ "/bin/bash", "-l" ]
