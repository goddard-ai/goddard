#!/usr/bin/env python3
import sys,json,os
with open('provider-starts.txt','a') as f: f.write(str(os.getpid())+'\n')
with open('provider-environment.json','w') as f: json.dump(sorted(k for k in os.environ if k.startswith('GODDARD_') or k=='WAKU_STABLE_RUNTIME_ROOT'),f)
thread='fixture-thread'
turn='native-turn'
def emit(v): print(json.dumps(v),flush=True)
def notice(m,p): emit({'method':m,'params':p})
def request(i,m): emit({'id':i,'method':m,'params':{'threadId':thread,'turnId':turn,'tool':'runtime_probe','arguments':{},'callId':'call'}})
for line in sys.stdin:
    v=json.loads(line)
    with open('received.jsonl','a') as f: f.write(json.dumps(v)+'\n')
    method=v.get('method')
    if method=='initialize': emit({'id':v['id'],'result':{}})
    elif method=='thread/start': emit({'id':v['id'],'result':{'thread':{'id':thread}}})
    elif method=='turn/start':
        emit({'id':v['id'],'result':{'turn':{'id':turn}}})
        notice('turn/started',{'threadId':thread,'turn':{'id':turn,'status':'inProgress'}})
        notice('thread/settings/updated',{'threadId':thread,'threadSettings':{'model':v['params'].get('model','native-default'),'effort':v['params'].get('effort','medium')}})
        notice('item/agentMessage/delta',{'threadId':thread,'turnId':'foreign-turn','delta':'must-ignore-foreign'})
        text=v['params']['input'][0]['text']
        if text=='oversize':
            print('x'*1048577,flush=True)
        elif text=='cancel':
            notice('item/agentMessage/delta',{'threadId':thread,'turnId':turn,'delta':'waiting'})
        elif text=='abandon':
            request(77,'item/tool/call')
            notice('serverRequest/resolved',{'threadId':thread,'requestId':77})
            request(78,'item/commandExecution/requestApproval')
            notice('item/agentMessage/delta',{'threadId':thread,'turnId':turn,'delta':'abandoned'})
            notice('turn/completed',{'threadId':thread,'turn':{'id':turn,'status':'completed'}})
        else:
            notice('item/agentMessage/delta',{'threadId':thread,'turnId':turn,'delta':'before'})
            request(11,'item/tool/call')
    elif method=='turn/interrupt':
        emit({'id':v['id'],'result':{}})
        notice('turn/completed',{'threadId':thread,'turn':{'id':turn,'status':'interrupted'}})
    elif method is None:
        i=v['id']
        notice('serverRequest/resolved',{'threadId':thread,'requestId':i})
        if i==11:
            notice('item/completed',{'threadId':thread,'turnId':turn,'item':{'type':'dynamicToolCall','id':'call','status':'completed','success':v['result']['success'],'contentItems':v['result']['contentItems']}})
        if i==11: request('11','item/commandExecution/requestApproval')
        elif i=='11': request('input','item/tool/requestUserInput')
        else:
            notice('item/agentMessage/delta',{'threadId':thread,'turnId':turn,'delta':'after'})
            notice('turn/completed',{'threadId':thread,'turn':{'id':turn,'status':'completed'}})
