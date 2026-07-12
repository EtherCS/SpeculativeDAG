# Copyright(C) Facebook, Inc. and its affiliates.
from os.path import join

from benchmark.utils import PathMaker


class CommandMaker:

    @staticmethod
    def cleanup():
        return (
            f'rm -r .db-* ; rm .*.json ; mkdir -p {PathMaker.results_path()}'
        )

    @staticmethod
    def clean_logs():
        return f'rm -r {PathMaker.logs_path()} ; mkdir -p {PathMaker.logs_path()}'

    @staticmethod
    def compile():
        return 'cargo build --quiet --release --features benchmark'

    @staticmethod
    def generate_key(filename):
        assert isinstance(filename, str)
        return f'./node generate_keys --filename {filename}'

    @staticmethod
    def run_primary(keys, committee, store, parameters, debug=False):
        assert isinstance(keys, str)
        assert isinstance(committee, str)
        assert isinstance(parameters, str)
        assert isinstance(debug, bool)
        v = '-vvv' if debug else '-vv'
        return (f'./node {v} run --keys {keys} --committee {committee} '
                f'--store {store} --parameters {parameters} primary')

    @staticmethod
    def run_worker(keys, committee, store, parameters, id, debug=False):
        assert isinstance(keys, str)
        assert isinstance(committee, str)
        assert isinstance(parameters, str)
        assert isinstance(debug, bool)
        v = '-vvv' if debug else '-vv'
        return (f'./node {v} run --keys {keys} --committee {committee} '
                f'--store {store} --parameters {parameters} worker --id {id}')

    @staticmethod
    def run_client(
        address,
        size,
        rate,
        nodes,
        workload=None,
        artifacts='.',
        num_clusters=5,
        families_per_cluster=5,
        people_per_family=8,
        replica_id=None,
        replica_num=None,
    ):
        assert isinstance(address, str)
        assert isinstance(size, int) and size > 0
        assert isinstance(rate, int) and rate >= 0
        assert isinstance(nodes, list)
        assert all(isinstance(x, str) for x in nodes)
        nodes = f'--nodes {" ".join(nodes)}' if nodes else ''
        evm = ''
        if workload:
            evm = (
                f' --workload {workload}'
                f' --artifacts {artifacts}'
                f' --num-clusters {num_clusters}'
                f' --families-per-cluster {families_per_cluster}'
                f' --people-per-family {people_per_family}'
            )
            if replica_id is not None:
                evm += f' --replica-id {replica_id}'
            if replica_num is not None:
                evm += f' --replica-num {replica_num}'
        return (
            f'./benchmark_client {address} --size {size} --rate {rate} {nodes}{evm}'
        )

    @staticmethod
    def kill():
        return 'tmux kill-server'

    @staticmethod
    def alias_binaries(origin):
        assert isinstance(origin, str)
        node, client = join(origin, 'node'), join(origin, 'benchmark_client')
        return (
            f'ln -sfn {node} node && '
            f'ln -sfn {client} benchmark_client && '
            'test -x ./node && test -x ./benchmark_client'
        )
