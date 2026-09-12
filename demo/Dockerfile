FROM python:3.12-alpine
RUN pip install --no-cache-dir nats-py==2.11.0
WORKDIR /demo
COPY traffic.py .
USER nobody
CMD ["python", "-u", "traffic.py"]
