FROM fedora AS builder
RUN echo "bump_005"

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
RUN alternatives --set java /usr/lib/jvm/java-21-openjdk/bin/java
RUN alternatives --set javac /usr/lib/jvm/java-21-openjdk/bin/javac

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

## FAST UPGRADE PATH
RUN echo bump_007
RUN /home/iphands/.opencode/bin/opencode upgrade
RUN npm install -g oh-my-opencode-linux-x64
RUN source /home/iphands/.bash_profile ; ./.bun/bin/bunx oh-my-opencode install --no-tui --claude=no --gemini=no --copilot=no --opencode-zen=no --zai-coding-plan=no

RUN ln -s /home/iphands/ocmonitor-share/ocmonitor-wrapper.sh /home/iphands/bin/ocm && \
    ln -s /home/iphands/.opencode/bin/opencode /home/iphands/bin/opencode || true

CMD [ "/bin/bash", "-l" ]
