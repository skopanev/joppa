#!/usr/bin/env python3
"""Check the existing reporter against authenticated MCP in an isolated database."""
import argparse
import importlib.util
from importlib.machinery import SourceFileLoader
import json
import os
from pathlib import Path
import secrets
import socket
import subprocess
import tempfile
import time
import urllib.parse
import uuid

root = Path(__file__).resolve().parents[1]
binary = os.environ.get('JOPPA_BIN', str(root / 'target/debug/joppa'))
database = os.environ['JOPPA_TEST_DATABASE_URL']
assert urllib.parse.urlparse(database).path.rsplit('/', 1)[-1].startswith('joppa_test')
env = {**os.environ, 'JOPPA_DATABASE_URL': database, 'JOPPA_ACTOR': 'owner'}
workspace = 'task-reporter-' + str(uuid.uuid4())
loader = SourceFileLoader('reporting_client', str(root / 'client/joppa'))
spec = importlib.util.spec_from_loader(loader.name, loader)
client = importlib.util.module_from_spec(spec)
loader.exec_module(client)

def seed(command):
    subprocess.run([binary, 'command', workspace, str(uuid.uuid4())], env=env,
                   input=json.dumps(command), text=True, check=True, stdout=subprocess.DEVNULL)

subprocess.run([binary, 'migrate'], env=env, check=True, stdout=subprocess.DEVNULL)
for command in [
    {'op':'workspace', 'title':'Task reporter fixture'},
    {'op':'member', 'id':'runner', 'role':'runner'},
    {'op':'domain', 'id':'d', 'title':'Checks', 'authority':'owner'},
    {'op':'capability', 'id':'c', 'title':'Reporting', 'domain':'d'},
    {'op':'consumer', 'id':'user', 'title':'Test contributor', 'owner':'owner'},
    {'op':'project', 'id':'app', 'repository':'https://example.test/fixture'},
    {'op':'revise', 'req':'r', 'expected_revision':0, 'capability':'c', 'consumers':['user'],
     'title':'Report exact outcomes', 'body':'Use real isolated process outcomes.', 'owner':'owner',
     'reason':'Set the isolated reporting contract', 'acs':[{'id':'a','text':'The process returns its actual outcome.'}]},
    {'op':'assess', 'req':'r', 'revision':1, 'ac':'a', 'assessment':{'ready':True,'reason':'The fixture process has an exact result.'}},
    {'op':'define_verification_scope', 'req':'r', 'revision':1, 'subject_keys':['app','environment'],
     'scope':'Actual fixture process exit status', 'reason':'Define the isolated fixture subject'},
    {'op':'save_object', 'kind':'task', 'id':'one', 'revision':0, 'parent':'ac:a',
     'data':{'title':'Fixture work','owner':'owner','contract':'Report the exact fixture process result.',
             'purpose':'implementation','projects':['app']}, 'reason':'Create a test binding target'},
]:
    seed(command)

with tempfile.TemporaryDirectory(prefix='joppa-task-reporter-') as temp:
    directory = Path(temp)
    token = secrets.token_urlsafe(32)
    tokens = directory / 'tokens.json'
    tokens.write_text(json.dumps({token:'runner'}))
    tokens.chmod(0o600)
    bearer = directory / 'bearer'
    bearer.write_text(token)
    bearer.chmod(0o600)
    with socket.socket() as sock:
        sock.bind(('127.0.0.1', 0))
        port = sock.getsockname()[1]
    url = f'http://127.0.0.1:{port}/mcp'
    process = subprocess.Popen([binary, 'serve', str(port)], env={**env,'JOPPA_TOKENS_FILE':str(tokens)},
                               stdout=subprocess.DEVNULL, stderr=subprocess.PIPE)
    os.environ['JOPPA_TOKEN_FILE'] = str(bearer)
    os.environ['JOPPA_MCP_URL'] = url
    os.environ.pop('JOPPA_TOKEN', None)
    args = argparse.Namespace(url=url, token_file=str(bearer))
    def call(name, **arguments):
        return client.mcp(args, name, {'workspace':workspace, **arguments})
    try:
        for attempt in range(50):
            try:
                obj = call('joppa_objects', address='task:one')
                break
            except client.JoppaError:
                if process.poll() is not None:
                    raise AssertionError('isolated MCP server failed to start')
                time.sleep(0.1)
        else:
            raise AssertionError('isolated MCP server did not start')
        origin = obj['process']['origin_revision']
        manifest = {'req':'r','revision':1,'subject':{'app':'fixture-build-1','environment':'isolated'},
                    'scope':'Actual fixture process exit status','discovery_complete':True,
                    'checks':[{'id':'[a] exact process','ac':'a','method':'automated','expected':['exit code 0'],
                               'tasks':[{'address':'task:one','origin_revision':origin}], 'command':['/bin/true']}]}
        filename = directory / 'checks.json'
        def report(expected_code):
            filename.write_text(json.dumps(manifest))
            result = subprocess.run(['python3',str(root/'scripts/run_checks.py'),workspace,str(filename),
                                     '--joppa',str(root/'client/joppa')],
                                    env={**os.environ,'JOPPA_TOKEN_FILE':str(bearer),'JOPPA_MCP_URL':url},
                                    text=True,capture_output=True)
            assert result.returncode == expected_code, result.stderr
            receipt = json.loads(result.stdout)
            assert receipt['passed'] == (expected_code == 0)
            return receipt
        success = report(0)
        passed = call('joppa_objects', address='task:one')
        assert passed['process']['verification'] == 'pass'
        assert passed['process']['status'] == 'open'
        assert call('joppa_read',req='r')['requirement']['revisions'][0]['confirmed'] is False
        manifest['checks'][0]['command'] = ['/bin/false']
        failure = report(1)
        failed = call('joppa_objects', address='task:one')
        assert failed['process']['verification'] == 'fail'
        assert failed['process']['status'] == 'open'
        source = call('joppa_read',req='r')['requirement']['revisions'][0]
        assert source['runs'][success['run']]['results']['[a] exact process']['outcome'] == 'pass'
        assert source['runs'][failure['run']]['results']['[a] exact process']['outcome'] == 'fail'
        assert source['runs'][failure['run']]['plan'][0]['tasks'][0]['origin_revision'] == origin
        assert source['runs'][failure['run']]['started'] < source['runs'][failure['run']]['finished']
        print(json.dumps({'authenticated_mcp':True,'real_pass_and_fail':True,'stable_binding':True,
                          'work_status_preserved':True,'confirmation_preserved':True,'history_preserved':True}))
    finally:
        process.terminate()
        process.wait(timeout=10)
