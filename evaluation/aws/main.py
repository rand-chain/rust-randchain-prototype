import boto3
import socket
import json
import collections
import dataclasses
import enum
import itertools
import os
import socket
import sys
import time
import pssh.clients
import gevent
import subprocess
import random
import math
from pprint import pprint
from datetime import datetime, timedelta
from typing import Any, Callable, Dict, Iterable, List, Optional, Union


################################################################
# Parameters


NUM_NODES = 16
REFRESH_INTERVAL = 5.0
REGIONS = {
    'us-east-1': 'N. Virginia',
    'us-west-1': 'N. California',
    'ap-south-1': 'Mumbai',
    'ap-northeast-2': 'Seoul',
    'ap-southeast-2': 'Sydney',
    'ap-northeast-1': 'Tokyo',
    'ca-central-1': 'Canada',
    'eu-west-1': 'Ireland',
    'sa-east-1': 'Sao Paulo',
    'us-west-2': 'Oregon',
    'us-east-2': 'Ohio',
    'ap-southeast-1': 'Singapore',
    'eu-west-2': 'London',
    'eu-central-1': 'Frankfurt',
}

TESTING = 0
if TESTING == 1:
    REGIONS = {
        'us-east-1': 'N. Virginia',
        'us-west-1': 'N. California',
        'ap-south-1': 'Mumbai',
    }
    NUM_NODES = 3
elif TESTING == 2:
    REGIONS = {
        'us-east-1': 'N. Virginia',
    }
    NUM_NODES = 32

INSTANCE_COUNT_PER_REGION: Dict[str, int] = collections.defaultdict(int)
_t_num_nodes = NUM_NODES
while _t_num_nodes:
    for r in REGIONS:
        INSTANCE_COUNT_PER_REGION[r] += 1
        _t_num_nodes -= 1
        if _t_num_nodes == 0:
            break

AMI_IMAGE_ID_PER_REGION: Dict[str, str] = {}

AWS_DIR = os.path.abspath(os.path.dirname(__file__))
NODES_PATH = os.path.abspath(os.path.join(AWS_DIR, 'nodes.txt'))
PACK_SCRIPT_PATH = os.path.join(AWS_DIR, "pack.sh")
SETUP_INSTANCE_SCRIPT_PATH = os.path.join(AWS_DIR, "setup-instance.sh")
with open(SETUP_INSTANCE_SCRIPT_PATH, 'r') as f:
    SETUP_INSTANCE_SCRIPT = f.read()

DATA_PATH = os.path.join(AWS_DIR, '..', 'data')
LOG_PATH = os.path.join(AWS_DIR, '..', 'log')
RESULTS_PATH = os.path.join(AWS_DIR, '..', 'data', 'results.csv')


################################################################
# utils

def Popen(args, stdout=subprocess.PIPE, stderr=subprocess.PIPE, **kwargs):
    p = subprocess.Popen(args, stdout=stdout, stderr=stderr, **kwargs)
    output, _ = p.communicate()
    assert p.returncode == 0, f"failed to execute {args}"
    return output.decode().strip()


def load_ami_image_ids():
    if AMI_IMAGE_ID_PER_REGION:
        return
    print()
    for region in REGIONS:
        print(
            f"{REGIONS[region] + ':': <41} search for amazon machine image... ", end="", flush=True)
        images = ec2[region].images.filter(
            Owners=['amazon'],
            Filters=[{
                'Name': 'name',
                'Values': ['amzn2-ami-hvm-2.0.????????-x86_64-gp2'],
            }])
        image = sorted(images, key=lambda i: i.creation_date)[-1]
        AMI_IMAGE_ID_PER_REGION[region] = image.id
        print(f"done ({image.id})")


class DryRunHandler:

    def __init__(self, dryrun=False):
        self.dryrun = dryrun

    def __enter__(self):
        return self

    def __exit__(self, exc_type, exc_value, exc_traceback):
        # returning False reraising any exception passed to this function
        if exc_value is None:
            assert not self.dryrun
            return True
        if self.dryrun:
            return 'DryRunOperation' in str(exc_value)
        return False

################################################################
# definitions on instances


class InstanceState(enum.IntEnum):
    PENDING = enum.auto()
    RUNNING = enum.auto()
    SHUTTING_DOWN = enum.auto()
    TERMINATED = enum.auto()
    STOPPING = enum.auto()
    STOPPED = enum.auto()

    @staticmethod
    def parse(name):
        return InstanceState[name.upper().replace("-", "_")]


class Instance:
    def __init__(self, id: str, region: str, dnsname: str = None, state: InstanceState = None):
        self.id = id
        self.region = region
        self.ssh_ok = False
        self.status = None
        self.state = state
        self.raw_info = None
        self.raw_status = None
        self.dnsname = dnsname or None
        self.public_ip = None

    @property
    def state(self):
        return self._state

    @state.setter
    def state(self, value):
        if value == InstanceState.RUNNING:
            self.ssh_ok = self.ssh_ok
        else:
            self.ssh_ok = False
        self._state = value

    def load_properties(self, instance_dict, status_dict):
        if self.id:
            assert self.id == instance_dict['InstanceId']
        else:
            self.id = instance_dict['InstanceId']
        self.raw_info = instance_dict
        self.raw_status = status_dict
        self.dnsname = instance_dict['PublicDnsName']
        self.state = InstanceState.parse(instance_dict['State']['Name'])
        self.status = status_dict['InstanceStatus']['Status'] if status_dict else None

    def __repr__(self):
        return (
            f"Instance(id='{self.id}', region='{self.region}', dnsname='{self.dnsname}', "
            f"state='{self.state.name}', ssh_ok='{self.ssh_ok}')"
        )


class Instances:
    def __init__(self, instances_dict: Dict[str, Instance] = None):
        self._instances_dict: Dict[str, Instance] = instances_dict or {}

    @property
    def ids(self):
        return [key for key in self._instances_dict]

    @property
    def all(self):
        return self

    @property
    def running(self):
        return Instances({i.id: i for i in self._instances_dict.values() if i.state == InstanceState.RUNNING})

    @property
    def pending(self):
        return Instances({i.id: i for i in self._instances_dict.values() if i.state == InstanceState.PENDING})

    @property
    def stopped(self):
        return Instances({i.id: i for i in self._instances_dict.values() if i.state == InstanceState.STOPPED})

    @property
    def stopping(self):
        return Instances({i.id: i for i in self._instances_dict.values() if i.state == InstanceState.STOPPING})

    @property
    def terminated(self):
        return Instances({i.id: i for i in self._instances_dict.values() if i.state == InstanceState.TERMINATED})

    def __len__(self):
        return len(self._instances_dict)

    def __getitem__(self, index_or_key: Union[int, str]):
        if isinstance(index_or_key, int):
            if index_or_key < 0:
                index_or_key += len(self._instances_dict)
            for i, value in enumerate(self._instances_dict.values()):
                if i == index_or_key:
                    return value
            raise IndexError
        else:
            return self._instances_dict[index_or_key]

    def __setitem__(self, key: str, value: Instance):
        if key in self._instances_dict:
            raise KeyError(
                'Cannot set the same instance twice, use refresh_infos() to update the existing instance.')
        self._instances_dict[key] = value

    def __repr__(self):
        return repr(list(self._instances_dict.values()))

    def get(self, key, default=None):
        return self._instances_dict.get(key, default)

    def by_region(self, return_dict=False):
        d = collections.defaultdict(Instances)
        for item in self._instances_dict.values():
            d[item.region][item.id] = item
        if return_dict:
            return d
        return d.items()

    def get_by_region(self, region):
        return [i for i in self if i.region == region]

    def lookup(self, what):
        if isinstance(what, Instances):
            return what
        if isinstance(what, Instance):
            return Instances({what.id: what})
        if isinstance(what, str) or isinstance(what, int):
            i = self[what]
            return Instances({i.id: i})
        if isinstance(what, Iterable):
            d = {}
            for x in what:
                i = x if isinstance(x, Instance) else self[x]
                d[i.id] = i
            return Instances(d)
        raise TypeError()

    def get_id(self, dnsname):
        for i in self:
            if i.dnsname == dnsname:
                return i.id
        raise ValueError(f"no instance with dnsname {dnsname} found")

    def status(self):
        self.refresh()
        print(f"number of running instances: {len(self.running)}")
        for x, v in self.running.by_region():
            print(f"    {x} {REGIONS[x]}: {len(v)}")

    def refresh(self, what=None):
        """ Uses the AWS API to refresh the information for all instances, considering all regions.
            If the parameter 'what' is provided, only the specified instances are queried.
        """
        if what is None:
            what = self
            grouped_instances = zip(REGIONS, itertools.repeat(None))
        else:
            what = self.lookup(what)
            grouped_instances = what.by_region()

        for region, instances_per_region in grouped_instances:
            ids = [] if instances_per_region is None else instances_per_region.ids
            infos = []
            for reservation in ec2_clients[region].describe_instances(InstanceIds=ids)['Reservations']:
                for i in reservation['Instances']:
                    infos.append(i)
            statuses = {}
            for status in ec2_clients[region].describe_instance_status(InstanceIds=ids)['InstanceStatuses']:
                statuses[status['InstanceId']] = status
            for info in infos:
                i = what.get(info['InstanceId'])
                if not i:
                    assert self.get(info['InstanceId']
                                    ) is None and what is self
                    i = Instance(info['InstanceId'], region)
                    self[i.id] = i
                i.load_properties(info, statuses.get(i.id))
        self.load_peers()

    def refresh_until(self, break_condition: Callable[[], bool], verbose: bool = True):
        while not break_condition():
            time.sleep(REFRESH_INTERVAL)
            self.refresh()
            if verbose:
                print(end=".", flush=True)

    def load_peers(self):
        for i in self.running:
            i.public_ip = socket.gethostbyname(i.dnsname)

    def get_peers(self):
        return [f"{i.public_ip}:8333" for i in self.running]

    def wait_for_startup(self, what=None):
        what = self if what is None else self.lookup(what)
        print(f"waiting for startup...", end='', flush=True)
        self.refresh_until(lambda: all(i.ssh_ok for i in what))
        print(" done")

    def start(self, what=None, dryrun=False):
        what = self.stopped if what is None else self.lookup(what)
        if not all(i.state == InstanceState.STOPPED for i in what):
            raise ValueError("instance(s) in invalid state")

        print(
            f"starting instance(s): {', '.join(what.ids)}...", end='', flush=True)
        for region, self in what.by_region():
            with DryRunHandler(dryrun):
                ec2_clients[region].start_instances(
                    InstanceIds=self.ids, DryRun=dryrun)
            if not dryrun:
                for i in self:
                    i.state = InstanceState.PENDING
        if not dryrun:
            self.refresh_until(lambda: all(i.ssh_ok for i in what))
        print(" done")

    def stop(self, what=None, dryrun=False):
        what = self.running if what is None else self.lookup(what)
        if not all(i.state == InstanceState.RUNNING for i in what):
            raise ValueError("instance(s) in invalid state")
        print(
            f"stopping instance(s): {', '.join(what.ids)}...", end='', flush=True)
        for region, self in what.by_region():
            with DryRunHandler(dryrun):
                ec2_clients[region].stop_instances(
                    InstanceIds=self.ids, DryRun=dryrun)
            if not dryrun:
                for i in self:
                    i.state = InstanceState.STOPPING
        if not dryrun:
            self.refresh_until(
                lambda: all(
                    i.state in [InstanceState.STOPPED, InstanceState.TERMINATED] for i in what)
            )
        print(" done")

    def terminate(self, what=None, dryrun=False):
        if what is None:
            what = [i for i in self if i.state != InstanceState.TERMINATED]
        what = self.lookup(what)
        print(
            f"terminating instance(s): {', '.join(what.ids)}...", end='', flush=True)
        for region, self in what.by_region():
            with DryRunHandler(dryrun):
                ec2_clients[region].terminate_instances(
                    InstanceIds=self.ids, DryRun=dryrun)
            if not dryrun:
                for i in self:
                    i.state = InstanceState.SHUTTING_DOWN
        if not dryrun:
            self.refresh_until(lambda: all(
                i.state == InstanceState.TERMINATED for i in what))
        print(" done")

    def create(self, instance_count_per_region=None, dryrun=False):
        if instance_count_per_region is None:
            instance_count_per_region = INSTANCE_COUNT_PER_REGION

        load_ami_image_ids()

        instance_count_per_region = {
            rid: ctr for rid, ctr in instance_count_per_region.items() if ctr > 0}
        running_by_region = self.running.by_region(return_dict=True)

        to_launch = 0
        print()
        print("launch initiated, aiming to launch the following instances:")
        for region in instance_count_per_region:
            count = instance_count_per_region[region]
            count = max(0, count - len(running_by_region.get(region, [])))
            to_launch += count
            instance_count_per_region[region] = count
            print(f"    {REGIONS[region] + ':': <41} {count: >3}")

        print()
        print(
            f"number of currecly running instances:         {len(self.running): >3}")
        print(f"total number of instance to launch:           {to_launch: >3}")
        print(
            f"total number of instance after launch:        {len(self.running) + to_launch: >3}")
        print()
        try:
            r = input("type 'y' and press enter to continue: ")
            if r != 'y':
                print('aborted')
                return
        except KeyboardInterrupt:
            print('\naborted')
            return

        print()
        print("performing launch...")
        print()
        for region, count in instance_count_per_region.items():
            if count > 0:
                Instances._create(region, count, dryrun=dryrun)

        time.sleep(1)
        self.refresh()

    @classmethod
    def _create(cls, region, num_instances=1, instance_type='t2.micro', dryrun=False):
        assert instance_type in ['t2.nano',
                                 't2.micro', 't2.small', 't2.medium']
        # assert num_instances <= 20, "check instance limits (10 for t2.micro, 20 for t2.small/t2.medium"

        load_ami_image_ids()

        print(
            f"    {REGIONS[region] + ':': <41} launching {num_instances: >3} instances... ", end="", flush=True)
        result = None
        with DryRunHandler(dryrun):
            result = ec2[region].create_instances(
                ImageId=AMI_IMAGE_ID_PER_REGION[region],
                InstanceType='t2.micro',
                KeyName='randchain',
                MinCount=num_instances,
                MaxCount=num_instances,
                UserData=SETUP_INSTANCE_SCRIPT,
                SecurityGroups=['randchain'],
                InstanceInitiatedShutdownBehavior='terminate',
                IamInstanceProfile={
                    # 'Arn': 'arn:aws:iam::290229848066:role/SystemManager',
                    'Name': 'SystemManager',
                },
                DryRun=dryrun,
            )
        print("done")
        return result


def test_ssh_connection(instances):
    if not all(i.dnsname for i in instances):
        raise ValueError(
            "instance(s) in invalid state, dnsname(s) not available")
    for i in instances:
        try:
            s = socket.socket(socket.AF_INET, socket.SOCK_STREAM)
            s.settimeout(REFRESH_INTERVAL)
            s.connect((i.dnsname, 22))
            s.shutdown(socket.SHUT_RDWR)
        except socket.timeout:
            return False
        except ConnectionError as e:
            print(e)
            return False
        finally:
            s.close()
    return True


####################################################################################
# Security group, key pair and node list stuff

def assign_security_group_to_all_instances(group_name):
    # directly set at launch of instance for now
    raise NotImplementedError


def create_or_update_security_groups():
    for region, region_name in REGIONS.items():
        for g in ec2[region].security_groups.all():
            if g.group_name == 'randchain':
                print(f"{region_name}: deleting security group...",
                      end='', flush=True)
                g.delete()
                print(" done")

        print(f"{region_name}: creating new security group...",
              end='', flush=True)
        g = ec2[region].create_security_group(
            GroupName='randchain', Description='Randchain security group (script generated)')
        print(" done")

        print(f"{region_name}: updating permissions...", end='', flush=True)
        g.authorize_ingress(
            FromPort=22,
            ToPort=22,
            IpProtocol='tcp',
            CidrIp='0.0.0.0/0',
        )
        g.authorize_ingress(
            FromPort=8333,
            ToPort=8333,
            IpProtocol='tcp',
            CidrIp='0.0.0.0/0',
        )
        print(" done")


####################################################################################
# SSH stuff

@dataclasses.dataclass
class SSHResult:
    id: int
    dnsname: str
    exit_code: int
    stdout: List[str]
    stderr: List[str]
    stdin: List[str]
    error: Any

    def __str__(self):
        if self.exit_code == 0:
            return self.stdout
        return f"ERROR({self.exit_code}): {self.stderr}"

    def __repr__(self):
        return f"SSHResult({repr(self.id)}, {repr(str(self))})"


class Operator:
    def __init__(self, instances, ssm_clients):
        self.instances = instances.running
        self.ssm_clients = ssm_clients
        self.ssh_client: pssh.clients.ParallelSSHClient = None

    def _run_command(self, cmd):
        command_ids = dict()
        for region in REGIONS:
            instanceIds = self.instances.by_region(
                return_dict=True)[region].ids
            response = self.ssm_clients[region].send_command(
                InstanceIds=instanceIds,
                DocumentName="AWS-RunShellScript",
                Parameters={'commands': [cmd]},
            )
            command_id = response['Command']['CommandId']
            command_ids[region] = command_id
        return command_ids

    def _get_outputs(self, command_ids):
        outputs = dict()
        for region in REGIONS:
            instanceIds = self.instances.by_region(
                return_dict=True)[region].ids
            for iid in instanceIds:
                output = self.ssm_clients[region].get_command_invocation(
                    CommandId=command_ids[region],
                    InstanceId=iid,
                )
                outputs[iid] = output
        return outputs

    def run_command_blocking(self, cmd):
        cids = self._run_command(cmd)
        outputs = self._get_outputs(cids)
        while True:
            finished = True
            for iid, out in outputs.items():
                if out['ResponseCode'] == -1:
                    finished = False
            if finished == True:
                break
            else:
                time.sleep(5)
                outputs = self._get_outputs(cids)
        return outputs

    def refresh(self):
        self.instances.status()
        self.instances = self.instances.running

    def if_deployed(self):
        cmd = "ls /home/ec2-user/main.sh"
        outputs = self.run_command_blocking(cmd)
        for iid, out in outputs.items():
            if out['ResponseCode'] == 0:
                print(f"{out['InstanceId']}: Deployed")
            else:
                print(f"{out['InstanceId']}: Not deployed yet, please wait")
        print()

    def run_benchmark(self, blocktime=10, num_miners=1):
        self.stop_benchmark()
        self.clean_logs()

        time.sleep(5)

        peers_str = ','.join(self.instances.get_peers())
        cmd = f'/home/ec2-user/main.sh {blocktime} {len(self.instances.running)} {num_miners} {peers_str}'

        print("Starting randchaind with command:\n %s" % cmd)
        self._run_command(cmd)

        print("done")

    def stop_benchmark(self, blocking=False):
        print("Killing randchaind processes")
        cmd = "pkill -9 -f randchaind & pkill -9 -f dstat"
        if blocking == False:
            self._run_command(cmd)
        else:
            outputs = self.run_command_blocking(cmd)
            for iid, out in outputs.items():
                if out['ResponseCode'] == 0:
                    print("Done at %s" % iid)
                else:
                    print("Error (or already done) at %s: %s" %
                          iid, out['StandardOutputContent'])

    def collect_logs(self, blocktime=10, num_miners=1, long=False):
        os.makedirs(LOG_PATH, exist_ok=True)
        log_dir = f"{LOG_PATH}/{blocktime}_{NUM_NODES}_{num_miners}"
        if long == True:
            log_dir += "_long"
        os.makedirs(log_dir, exist_ok=True)
        print("Collecting logs")
        procs = []
        for idx, i in enumerate(self.instances.running):
            for remote_path in ['/home/ec2-user/main.log', '/home/ec2-user/stats.csv']:
                file_fullpath = f'{log_dir}/{i.dnsname}_{remote_path.split("/")[-1]}'
                if os.path.exists(file_fullpath):
                    continue
                print(
                    f"Downloading {remote_path} from {i.dnsname+'...': <65} {idx + 1}/{len(self.instances.running)} ")
                # Filename: dnsname_blocktime_numnodes_numminers_{stats.csv/main.log}
                cmd = ' '.join([
                    f'rsync -z -e "ssh -i ~/.ssh/randchain.pem -oStrictHostKeyChecking=accept-new"',
                    f'ec2-user@{i.dnsname}:{remote_path}',
                    f'"{file_fullpath}"',
                ])
                proc = subprocess.Popen(
                    cmd, shell=True, stderr=subprocess.DEVNULL)
                procs.append(proc)

        # collect status of all subprocesses
        outputs = [p.wait() for p in procs]
        print("Done")

    def clean_logs(self, blocking=False):
        print("Removing logs")
        cmd = "rm -rf /home/ec2-user/stats.csv /home/ec2-user/main.log /home/ec2-user/.local/share/randchaind/"
        if blocking == False:
            self._run_command(cmd)
        else:
            outputs = self.run_command_blocking(cmd)
            for iid, out in outputs.items():
                if out['ResponseCode'] == 0:
                    print("Done at %s" % iid)
                else:
                    print("Error (or already done) at %s: %s" %
                          iid, out['StandardOutputContent'])


if __name__ == '__main__':
    if len(sys.argv) == 2:
        NUM_NODES = int(sys.argv[1])
    print(f"setting NUM_NODES={NUM_NODES}")

    ec2 = {
        region: boto3.resource("ec2", region_name=region) for region in REGIONS
    }
    ec2_clients = {
        region: boto3.client("ec2", region_name=region) for region in REGIONS
    }
    ssm_clients = {
        region: boto3.client("ssm", region_name=region) for region in REGIONS
    }

    # security group
    # create_or_update_security_groups()

    instances = Instances()
    # instances.create()

    instances.refresh()
    instances.status()
    # instances.running

    # Here you need to wait for some time (e.g., 1 min) until `setup-instance.sh` is executed on all instances

    op = Operator(instances, ssm_clients)
    # print(op.ssm_clients['us-east-1'].send_command(InstanceIds=['i-0945ba88c51f82960'],
    #                                                DocumentName="AWS-RunShellScript", Parameters={'commands': ['echo hellofuckyou']}))
    # op.ssh_connect(instances)
    # op.run_benchmark(instances)
    # op.collect_logs(instances)

    # instances.stop()
    # instances.terminate()
