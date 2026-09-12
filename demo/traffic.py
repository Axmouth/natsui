"""Dedicated demo workload. Never accepts an external NATS endpoint."""
import asyncio
import json
import time
import uuid
from pathlib import Path
import nats
from nats.errors import TimeoutError
from nats.js.api import StreamConfig, ConsumerConfig, AckPolicy, RetentionPolicy

SERVERS = [f'nats://nats-{i}:4222' for i in range(1, 4)]
RESOURCES = [('ORDERS', 'orders.created'), ('PAYMENTS', 'payments.captured'), ('JOBS', 'jobs.resize')]
started = time.monotonic()
run_id = uuid.uuid4().hex[:8]


def phase():
    tick = int(time.monotonic() - started) % 120
    return (0 if tick < 20 else 1 if tick < 40 else 2 if tick < 60 else 3)


async def connect(name):
    return await nats.connect(SERVERS, name=f'natsui-demo/{name}', max_reconnect_attempts=-1)


async def publish(name, subject, rates):
    nc = await connect(f'producer/{name}')
    js = nc.jetstream()
    seq = 0
    while True:
        begin = time.monotonic()
        rate = rates[phase()]
        # Small concurrent batches wait for JetStream persistence acknowledgments.
        for offset in range(0, rate, 20):
            batch = []
            for _ in range(min(20, rate - offset)):
                seq += 1
                payload = json.dumps(dict(demo=True, run=run_id, id=seq, type=name,
                                          produced_at=time.time(), scenario=phase())).encode()
                batch.append(js.publish(subject, payload))
            await asyncio.gather(*batch)
        await asyncio.sleep(max(0, 1 - (time.monotonic() - begin)))


async def worker(stream, consumer, rates, worker_id, retries=False):
    nc = await connect(f'worker/{worker_id}')
    sub = await nc.jetstream().pull_subscribe_bind(consumer=consumer, stream=stream)
    while True:
        begin = time.monotonic()
        try:
            messages = await sub.fetch(batch=rates[phase()], timeout=0.8)
        except TimeoutError:
            messages = []
        if messages:
            # Holding delivery before acknowledgment makes in-flight work observable.
            await asyncio.sleep(0.2 if phase() != 1 else 0.6)
            for msg in messages:
                meta = msg.metadata
                if retries and meta.num_delivered == 1 and meta.sequence.stream % 37 == 0:
                    await msg.nak(delay=2)
                else:
                    await msg.ack()
        await asyncio.sleep(max(0, 1 - (time.monotonic() - begin)))


async def core_worker(identity):
    nc = await connect(f'core-worker/{identity}')
    async def receive(msg):
        await asyncio.sleep(0.01)
    await nc.subscribe('demo.live.events', queue='live-workers', cb=receive)
    await asyncio.Event().wait()


async def core_producer():
    nc = await connect('core-producer')
    while True:
        for i in range(20):
            await nc.publish('demo.live.events', json.dumps(dict(demo=True, event=i)).encode())
        await nc.flush()
        await asyncio.sleep(1)


async def report(js):
    labels = ['Steady traffic', 'Slow billing', 'Producer burst', 'Recovery']
    while True:
        stats = {}
        for name, _ in RESOURCES:
            info = await js.stream_info(name)
            stats[name] = info.state.messages
        billing = await js.consumer_info('ORDERS', 'billing-worker')
        print(json.dumps(dict(phase=labels[phase()], retained=stats,
                              billing_pending=billing.num_pending, source='real JetStream')))
        Path('/tmp/traffic-heartbeat').write_text(str(time.time()))
        await asyncio.sleep(5)


async def main():
    global started
    nc = await connect('setup')
    js = nc.jetstream(timeout=10)
    for name, subject in RESOURCES:
        await js.add_stream(config=StreamConfig(name=name, subjects=[subject],
            retention=RetentionPolicy.WORK_QUEUE if name == 'JOBS' else RetentionPolicy.LIMITS,
            storage='file', num_replicas=3, max_msgs=100000, max_bytes=64*1024*1024,
            max_age=600, metadata={'natsui.demo': 'scripted traffic, real JetStream data'}))
    workers = [('ORDERS', 'billing-worker', [260,20,40,1000], True),
               ('ORDERS', 'fulfillment', [1000,1000,1000,1000], False),
               ('PAYMENTS', 'settlement', [100,30,40,300], True),
               ('PAYMENTS', 'receipts', [200,200,200,200], False),
               ('JOBS', 'image-workers', [100,20,30,500], True)]
    for stream, name, _, _ in workers:
        await js.add_consumer(stream, config=ConsumerConfig(durable_name=name,
            ack_policy=AckPolicy.EXPLICIT, ack_wait=5, max_ack_pending=2000, max_deliver=10))
    started = time.monotonic()
    async with asyncio.TaskGroup() as tasks:
        for stream, name, rates, retries in workers:
            tasks.create_task(worker(stream, name, rates, name, retries))
        tasks.create_task(worker('JOBS', 'image-workers', [80,20,30,500], 'image-workers-2', True))
        for (name, subject), rates in zip(RESOURCES, [[200,200,800,100], [80,80,120,60], [160,160,500,80]]):
            tasks.create_task(publish(name, subject, rates))
        tasks.create_task(core_worker('a'))
        tasks.create_task(core_worker('b'))
        tasks.create_task(core_producer())
        tasks.create_task(report(js))


asyncio.run(main())
