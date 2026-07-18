FROM fedora:44 AS builder
RUN echo "bump_008"

# Route dnf through the homelab pkgcache proxy (nginx reverse cache).
# See /home/iphands/prog/slop/cache/scripts/fix-fedora -- same approach: Fedora's
# stock repos use HTTPS + metalink, which a caching proxy can't serve, so we add
# explicit baseurl repos pointing at the cache and disable the metalink ones.
# gpgcheck stays on, so RPMs remain signature-verified over plain HTTP.
RUN sed -i 's/^enabled=1/enabled=0/' /etc/yum.repos.d/fedora*.repo && \
    cat > /etc/yum.repos.d/pkgcache.repo <<'EOF'
[fedora-cache]
name=Fedora $releasever - $basearch (cached via pkgcache)
baseurl=http://noir.lan:3129/fedora/linux/releases/$releasever/Everything/$basearch/os/
enabled=1
gpgcheck=1
gpgkey=file:///etc/pki/rpm-gpg/RPM-GPG-KEY-fedora-$releasever-$basearch
skip_if_unavailable=True

[updates-cache]
name=Fedora $releasever - $basearch - Updates (cached via pkgcache)
baseurl=http://noir.lan:3129/fedora/linux/updates/$releasever/Everything/$basearch/
enabled=1
gpgcheck=1
gpgkey=file:///etc/pki/rpm-gpg/RPM-GPG-KEY-fedora-$releasever-$basearch
skip_if_unavailable=True
EOF

RUN dnf group install -y development-tools

COPY .dnf-deps /dnf-deps
RUN dnf install -y `cat /dnf-deps | grep -v '#'`

# Drupal stuff
RUN php -r "copy('https://getcomposer.org/installer', 'composer-setup.php');"
RUN php -r "if (hash_file('sha384', 'composer-setup.php') === 'c8b085408188070d5f52bcfe4ecfbee5f727afa458b2573b8eaaf77b3419b0bf2768dc67c86944da1544f06fa544fd47') { echo 'Installer verified'.PHP_EOL; } else { echo 'Installer corrupt'.PHP_EOL; unlink('composer-setup.php'); exit(1); }"
RUN php composer-setup.php
RUN php -r "unlink('composer-setup.php');"
RUN mv composer.phar /usr/local/bin/composer
RUN echo "iphands ALL=(ALL) NOPASSWD: ALL" >> /etc/sudoers
RUN alternatives --set java /usr/lib/jvm/java-25-openjdk/bin/java
RUN alternatives --set javac /usr/lib/jvm/java-25-openjdk/bin/javac

RUN useradd iphands
USER iphands
WORKDIR /home/iphands

# Setup composer for Drupal
RUN composer require drush/drush
RUN mkdir ~/bin && ln -s ~/vendor/bin/drush ~/bin/drush

RUN curl https://pyenv.run | bash
RUN export PATH="$HOME/.pyenv/bin:$PATH" && \
    eval "$(pyenv init --path)" && \
    eval "$(pyenv init -)" && \
    pyenv install 3.12 && \
    pyenv global 3.12

ENV PATH="/home/iphands/.pyenv/bin:$PATH"
env PATH="/home/iphands/.pyenv/shims:${PATH}"

FROM builder
RUN git config --global user.name "Ian Page Hands" && git config --global user.email "iphands@gmail.com"
RUN echo bump_004
RUN touch /home/iphands/IM_A_CONTAINER

RUN sudo dnf remove -y rust
RUN yes 1 | rustup-init
RUN .cargo/bin/cargo install just
RUN sudo flatpak remote-add --if-not-exists flathub https://flathub.org/repo/flathub.flatpakrepo
RUN sudo flatpak install -y org.ghidra_sre.Ghidra

RUN curl -fsSL https://opencode.ai/install | bash
RUN curl -fsSL https://bun.com/install | bash

RUN npm config set prefix '~/.local/'
RUN npm install -g @ast-grep/cli
RUN npm install -g pyright typescript typescript-language-server

RUN git clone https://github.com/iphands/dotfiles.git && cd dotfiles && ./init
RUN git clone https://github.com/Shlomob/ocmonitor-share.git && cd ocmonitor-share && ./install.sh

RUN ln -s /home/iphands/prog/android           /home/iphands/

RUN source /home/iphands/.bash_profile ; rustup component add rust-analyzer

## FAST UPGRADE PATH
RUN echo bump_008
RUN /home/iphands/.opencode/bin/opencode upgrade
RUN npm install -g oh-my-opencode-linux-x64
RUN source /home/iphands/.bash_profile ; source /home/iphands/.bashrc ; echo "PATH=${PATH}:${HOME}/.bun/bin" >> .bashrc
RUN source /home/iphands/.bash_profile ; source /home/iphands/.bashrc ; which bun
RUN source /home/iphands/.bash_profile ; source /home/iphands/.bashrc ; ./.bun/bin/bunx oh-my-opencode install --no-tui --claude=no --gemini=no --copilot=no --opencode-zen=no --zai-coding-plan=no

RUN ln -s /home/iphands/ocmonitor-share/ocmonitor-wrapper.sh /home/iphands/bin/ocm && \
    ln -s /home/iphands/.opencode/bin/opencode /home/iphands/bin/opencode || true

CMD [ "/bin/bash", "-l" ]
