FROM python:3.12-alpine
RUN pip install --no-cache-dir nats-py==2.11.0
WORKDIR /demo
COPY traffic.py .
LABEL org.opencontainers.image.source="https://github.com/Axmouth/natsui" \
      org.opencontainers.image.description="Isolated Natsui demo traffic generator" \
      org.opencontainers.image.licenses="MIT"
USER nobody
CMD ["python", "-u", "traffic.py"]
