FROM fedora

RUN dnf install -y nodejs
RUN dnf install -y emacs-nox tree
RUN useradd iphands

USER iphands

RUN curl -fsSL https://pi.dev/install.sh | sh

WORKDIR /home/iphands

RUN echo 'export PATH="/home/iphands/.pi/agent/bin:${PATH}"' >> /home/iphands/.bashrc

CMD [ "/bin/bash", "-l" ]
