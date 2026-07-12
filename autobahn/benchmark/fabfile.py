# Copyright(C) Facebook, Inc. and its affiliates.
from fabric import task

from benchmark.local import LocalBench
from benchmark.logs import ParseError, LogParser
from benchmark.utils import Print
from benchmark.plot import Ploter, PlotError
from benchmark.instance import InstanceManager
from benchmark.remote import Bench, BenchError


def _make_bench_params(workload='erc20', artifacts='.', num_clusters=5,
                       families_per_cluster=5, people_per_family=8,
                       faults=0, nodes=4, workers=1, rate=400,
                       tx_size=512, duration=60, runs=1):
    return {
        'faults': int(faults),
        'nodes': [int(nodes)],
        'workers': int(workers),
        'co-locate': True,
        'rate': [int(rate)],
        'tx_size': int(tx_size),
        'duration': int(duration),
        'runs': int(runs),

        # EVM benchmark client config.
        'workload': None if workload in (None, '', 'none') else workload,
        'artifacts': artifacts,
        'num_clusters': int(num_clusters),
        'families_per_cluster': int(families_per_cluster),
        'people_per_family': int(people_per_family),

        # Legacy network-partition experiment; disabled for normal benchmarks.
        'simulate_partition': False,
        'partition_start': 5,
        'partition_duration': 5,
        'partition_nodes': 1,
    }


def _make_node_params(execution='none', workload='erc20', executor='sequential',
                      artifacts='.', num_clusters=5, families_per_cluster=5,
                      people_per_family=8, simulate_asynchrony=False,
                      asynchrony_start=15_000, asynchrony_duration=3_000,
                      simulate_order_stall=False, order_stall_start=10_000,
                      order_stall_duration=50_000):
    return {
        'timeout_delay': 5_000,  # ms
        'header_size': 32,  # bytes
        'max_header_delay': 5_000,  # ms
        'gc_depth': 50,  # rounds
        'sync_retry_delay': 5_000,  # ms
        'sync_retry_nodes': 3,  # number of nodes
        'batch_size': 500_000,  # bytes
        'max_batch_delay': 20,  # ms
        'use_optimistic_tips': True,
        'use_parallel_proposals': True,
        'k': 4,
        'use_fast_path': True,
        'fast_path_timeout': 5_000,
        'use_ride_share': False,
        'car_timeout': 5_000,

        'simulate_asynchrony': bool(simulate_asynchrony),
        'asynchrony_start': int(asynchrony_start), #ms
        'asynchrony_duration': int(asynchrony_duration), #ms
        'simulate_order_stall': bool(simulate_order_stall),
        'order_stall_start': int(order_stall_start),
        'order_stall_duration': int(order_stall_duration),

        # EVM execution config for primaries.
        'evm_execution_mode': execution,
        'evm_workload': workload,
        'evm_executor_mode': executor,
        'evm_artifacts_dir': artifacts,
        'evm_num_clusters': int(num_clusters),
        'evm_num_families_per_cluster': int(families_per_cluster),
        'evm_num_people_per_family': int(people_per_family),
    }


@task
def local(ctx, debug=True, execution='none', workload='erc20',
          executor='sequential', artifacts='.', num_clusters=5,
          families_per_cluster=5, people_per_family=8,
          simulate_asynchrony=False, asynchrony_start=15_000,
          asynchrony_duration=3_000, faults=0, nodes=4, workers=1,
          rate=400, tx_size=512, duration=60, runs=1):
    ''' Run benchmarks on localhost '''
    bench_params = _make_bench_params(
        workload=workload,
        artifacts=artifacts,
        num_clusters=num_clusters,
        families_per_cluster=families_per_cluster,
        people_per_family=people_per_family,
        faults=faults,
        nodes=nodes,
        workers=workers,
        rate=rate,
        tx_size=tx_size,
        duration=duration,
        runs=runs,
    )
    node_params = _make_node_params(
        execution=execution,
        workload=workload,
        executor=executor,
        artifacts=artifacts,
        num_clusters=num_clusters,
        families_per_cluster=families_per_cluster,
        people_per_family=people_per_family,
        simulate_asynchrony=simulate_asynchrony,
        asynchrony_start=asynchrony_start,
        asynchrony_duration=asynchrony_duration,
    )
    try:
        ret = LocalBench(bench_params, node_params).run(debug)
        print(ret.result())
    except BenchError as e:
        Print.error(e)


@task
def create(ctx, nodes=6):
    ''' Create a testbed'''
    try:
        InstanceManager.make().create_instances(nodes)
    except BenchError as e:
        Print.error(e)


@task
def destroy(ctx):
    ''' Destroy the testbed '''
    try:
        InstanceManager.make().terminate_instances()
    except BenchError as e:
        Print.error(e)


@task
def start(ctx, max=2):
    ''' Start at most `max` machines per data center '''
    try:
        InstanceManager.make().start_instances(max)
    except BenchError as e:
        Print.error(e)


@task
def stop(ctx):
    ''' Stop all machines '''
    try:
        InstanceManager.make().stop_instances()
    except BenchError as e:
        Print.error(e)


@task
def info(ctx):
    ''' Display connect information about all the available machines '''
    try:
        InstanceManager.make().print_info()
    except BenchError as e:
        Print.error(e)


@task
def install(ctx):
    ''' Install the codebase on all machines '''
    try:
        Bench(ctx).install()
    except BenchError as e:
        Print.error(e)


@task
def remote(ctx, debug=True, execution='ordered', workload='erc20',
           executor='sequential', artifacts='.', num_clusters=5,
           families_per_cluster=5, people_per_family=8,
           simulate_asynchrony=False, asynchrony_start=15_000,
           asynchrony_duration=3_000, faults=0, nodes=4, workers=1,
           rate=400, tx_size=512, duration=30, runs=1):
    ''' Run benchmarks on AWS '''
    bench_params = _make_bench_params(
        workload=workload,
        artifacts=artifacts,
        num_clusters=num_clusters,
        families_per_cluster=families_per_cluster,
        people_per_family=people_per_family,
        faults=faults,
        nodes=nodes,
        workers=workers,
        rate=rate,
        tx_size=tx_size,
        duration=duration,
        runs=runs,
    )
    node_params = _make_node_params(
        execution=execution,
        workload=workload,
        executor=executor,
        artifacts=artifacts,
        num_clusters=num_clusters,
        families_per_cluster=families_per_cluster,
        people_per_family=people_per_family,
        simulate_asynchrony=simulate_asynchrony,
        asynchrony_start=asynchrony_start,
        asynchrony_duration=asynchrony_duration,
    )
    try:
        Bench(ctx).run(bench_params, node_params, debug)
    except BenchError as e:
        Print.error(e)


@task
def remote_attack(ctx, debug=True, execution='ordered', workload='erc20',
                  executor='sequential', artifacts='.', num_clusters=5,
                  families_per_cluster=5, people_per_family=8,
                  order_stall_start=10_000, order_stall_duration=15_000,
                  faults=0, nodes=4, workers=1, rate=400, tx_size=512,
                  duration=60, runs=1):
    ''' Run a deterministic consecutive order-stall benchmark on AWS '''
    bench_params = _make_bench_params(
        workload=workload,
        artifacts=artifacts,
        num_clusters=num_clusters,
        families_per_cluster=families_per_cluster,
        people_per_family=people_per_family,
        faults=faults,
        nodes=nodes,
        workers=workers,
        rate=rate,
        tx_size=tx_size,
        duration=duration,
        runs=runs,
    )
    node_params = _make_node_params(
        execution=execution,
        workload=workload,
        executor=executor,
        artifacts=artifacts,
        num_clusters=num_clusters,
        families_per_cluster=families_per_cluster,
        people_per_family=people_per_family,
        simulate_order_stall=True,
        order_stall_start=order_stall_start,
        order_stall_duration=order_stall_duration,
    )
    try:
        Bench(ctx).run(bench_params, node_params, debug)
    except BenchError as e:
        Print.error(e)


@task
def plot(ctx):
    ''' Plot performance using the logs generated by "fab remote" '''
    plot_params = {
        'faults': [0],
        'nodes': [4],
        'workers': [1, 4, 7, 10],
        'collocate': True,
        'tx_size': 512,
        'max_latency': [2_000, 2_500]
    }
    try:
        Ploter.plot(plot_params)
    except PlotError as e:
        Print.error(BenchError('Failed to plot performance', e))


@task
def kill(ctx):
    ''' Stop execution on all machines '''
    try:
        Bench(ctx).kill()
    except BenchError as e:
        Print.error(e)


@task
def logs(ctx):
    ''' Print a summary of the logs '''
    try:
        directory = LogParser.latest_run_directory('./logs')
        Print.info(f'Parsing logs from {directory}')
        print(LogParser.process(directory, faults='?').result())
    except ParseError as e:
        Print.error(BenchError('Failed to parse logs', e))


@task
def jitter(ctx, debug=True, execution='ordered', workload='erc20',
           executor='sequential', artifacts='.', num_clusters=5,
           families_per_cluster=5, people_per_family=8,
           asynchrony_start=10_000, asynchrony_duration=50_000,
           faults=0, nodes=4, workers=1, rate=400, tx_size=512,
           duration=60, runs=1):
    ''' Run a local jitter/asynchrony benchmark on localhost '''
    bench_params = _make_bench_params(
        workload=workload,
        artifacts=artifacts,
        num_clusters=num_clusters,
        families_per_cluster=families_per_cluster,
        people_per_family=people_per_family,
        faults=faults,
        nodes=nodes,
        workers=workers,
        rate=rate,
        tx_size=tx_size,
        duration=duration,
        runs=runs,
    )
    node_params = _make_node_params(
        execution=execution,
        workload=workload,
        executor=executor,
        artifacts=artifacts,
        num_clusters=num_clusters,
        families_per_cluster=families_per_cluster,
        people_per_family=people_per_family,
        simulate_asynchrony=True,
        asynchrony_start=asynchrony_start,
        asynchrony_duration=asynchrony_duration,
    )
    try:
        ret = LocalBench(bench_params, node_params).run(debug)
        print(ret.result())
    except BenchError as e:
        Print.error(e)


@task
def attack(ctx, debug=True, execution='ordered', workload='erc20',
           executor='sequential', artifacts='.', num_clusters=5,
           families_per_cluster=5, people_per_family=8,
           order_stall_start=10_000, order_stall_duration=50_000,
           faults=0, nodes=4, workers=1, rate=400, tx_size=512,
           duration=60, runs=1):
    ''' Run a deterministic consecutive order-stall benchmark on localhost '''
    bench_params = _make_bench_params(
        workload=workload,
        artifacts=artifacts,
        num_clusters=num_clusters,
        families_per_cluster=families_per_cluster,
        people_per_family=people_per_family,
        faults=faults,
        nodes=nodes,
        workers=workers,
        rate=rate,
        tx_size=tx_size,
        duration=duration,
        runs=runs,
    )
    node_params = _make_node_params(
        execution=execution,
        workload=workload,
        executor=executor,
        artifacts=artifacts,
        num_clusters=num_clusters,
        families_per_cluster=families_per_cluster,
        people_per_family=people_per_family,
        simulate_order_stall=True,
        order_stall_start=order_stall_start,
        order_stall_duration=order_stall_duration,
    )
    try:
        ret = LocalBench(bench_params, node_params).run(debug)
        print(ret.result())
    except BenchError as e:
        Print.error(e)
