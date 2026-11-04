# OpenCode 2 (no oh-my-opencode, no ocmonitor) on top of ocli-base (../opencode/Dockerfile.base)
# Built by ./bin/build-container-opencode2
FROM ocli-base

## FAST UPGRADE PATH
RUN echo bump_002
# v2 installs as ~/.opencode/bin/opencode (plus an opencode2 shim); --no-modify-path: ~/bin symlinks below
RUN curl -fsSL https://opencode.ai/v2/install | bash -s -- --no-modify-path

# ~/bin/opencode2 is the raw binary; bin/ holds ~/bin/opencode (joins the container's
# `opencode serve`) and ~/bin/opencode-serve (runs that serve). COPY'd, not symlinked, so
# neither may already exist as a symlink here
RUN ln -s /home/iphands/.opencode/bin/opencode /home/iphands/bin/opencode2

COPY --chown=iphands:iphands bin/ /home/iphands/bin/
RUN chmod 755 /home/iphands/bin/opencode /home/iphands/bin/opencode-serve

CMD [ "/bin/bash", "-l" ]
