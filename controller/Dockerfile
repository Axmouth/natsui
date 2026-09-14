FROM nats:2.11.8-alpine AS runtime
RUN apk add --no-cache python3 py3-bcrypt && addgroup -g 10001 natsui && adduser -D -u 10001 -G natsui natsui && mkdir /data && chown 10001:10001 /data
COPY server.py /opt/natsui-controller/server.py
USER 10001:10001
EXPOSE 4222 6222 8222 9443
ENTRYPOINT ["python3", "/opt/natsui-controller/server.py"]

FROM runtime AS verification
USER root
RUN apk add --no-cache openssl
USER 10001:10001
COPY test_controller.py /opt/natsui-controller/test_controller.py
RUN python3 /opt/natsui-controller/test_controller.py

FROM runtime
