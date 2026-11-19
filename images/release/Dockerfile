# The images of the release binaries of podcrates on ghcr.io (call C789):
# one stage for each binary, FROM scratch, with the static musl binary of
# dist/ as the entrypoint. The job image of release.yml selects the stage
# with its target, and gives a context with NAME-amd64 and NAME-arm64.
# An exec-form ENTRYPOINT takes no build argument, so each binary has its
# own stage. COPY sets the mode: an artifact of a job loses it.

FROM scratch AS podlab
ARG TARGETARCH
COPY --chmod=0755 podlab-${TARGETARCH} /usr/local/bin/podlab
ENTRYPOINT ["/usr/local/bin/podlab"]

FROM scratch AS pod-fake-github
ARG TARGETARCH
COPY --chmod=0755 pod-fake-github-${TARGETARCH} /usr/local/bin/pod-fake-github
ENTRYPOINT ["/usr/local/bin/pod-fake-github"]

FROM scratch AS pod-fake-githome
ARG TARGETARCH
COPY --chmod=0755 pod-fake-githome-${TARGETARCH} /usr/local/bin/pod-fake-githome
ENTRYPOINT ["/usr/local/bin/pod-fake-githome"]

FROM scratch AS pod-release
ARG TARGETARCH
COPY --chmod=0755 pod-release-${TARGETARCH} /usr/local/bin/pod-release
ENTRYPOINT ["/usr/local/bin/pod-release"]
