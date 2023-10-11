import os
import threading
import time

import requests
import websocket
import json
from google.cloud import firestore

data = {}

db = firestore.Client()

session = requests.Session()

socket = f'wss://clientes.balanz.com/websocket'

user = os.environ.get('balanzuser')

password = os.environ.get('balanzpassword')

payload = {"data": {"user": user, "pass": password}}

user_agent = 'Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/99.0.4844.74 Safari/537.36 Edg/99.0.1150.46'

login_headers = {'Content-type': 'application/json', 'Accept': 'application/json', 'User-Agent': user_agent,
                 'Referer': 'https://clientes.balanz.com/'}

access_token_doc = db.collection(u'MEPBot').document(u'AccessToken')

access_token = access_token_doc.get().to_dict()['value']

msg = None


def login():
    global access_token, access_token_doc

    r = session.post('https://clientes.balanz.com/api/v1/login', json=payload,
                     headers=login_headers)

    access_token = r.json()['AccessToken']

    if access_token is not None and access_token != '':
        access_token_doc.set(
            {
                u'value': access_token
            }, merge=True)


def on_message(ws, message):
    message = json.loads(message)
    if message['plazo'] == 'CI':
        if message['ticker'] == 'AL30':
            data['al30_ask'] = message['pv'] * 100
        if message['ticker'] == 'AL30D':
            data['al30d_ask'] = message['pv'] * 100
        if message['ticker'] == 'AL30':
            data['al30_bid'] = message['pc'] * 100
        if message['ticker'] == 'GD30D':
            data['gd30d_ask'] = message['pv'] * 100
        if message['ticker'] == 'GD30':
            data['gd30_bid'] = message['pc'] * 100

    if message['plazo'] == '48hs':
        if message['ticker'] == 'AL30D':
            data['al30d_bid_48hs'] = message['pc'] * 100


    if {'al30d_ask', 'al30_bid', 'gd30d_ask', 'gd30_bid', 'al30_ask_48hs', 'al30d_bid_48hs'}.issubset(data.keys()):
        print(data)
        ws.on_close = None #otherwise we'd have to call login() every time
        ws.keep_running = False


def on_open(ws):
    ws.send(msg)
    print('Open stream')


def on_error(ws, exception):
    print('Stream error')
    print(exception)


def on_close(ws, status, message):
    login() # in case the authtoken expires
    print('Closed stream')
    print('status' + str(status))
    print('message' + str(message))
    if status is None and message is None:
        get_quotes(None)


def get_quotes(request):

    global msg
    global data

    data = {}

    msg = json.dumps({"panel": 6, "token": access_token})

    wss_header = {'User-Agent': user_agent, 'Origin': 'https://clientes.balanz.com'}

    websocket.setdefaulttimeout(30)

    wss = websocket.WebSocketApp(socket, on_message=on_message, on_open=on_open, on_error=on_error, on_close=on_close,
                                 header=wss_header)

    wst = threading.Thread(target=wss.run_forever)
    wst.daemon = True
    wst.start()
    wst.join(timeout=30)

    # if wst.is_alive():
    #     print('Timeout')
    #     wss.close()
    # time.sleep(30)
    #
    # if wss.keep_running:
    #     wss.keep_running = False

    return json.dumps(data)


if __name__ == '__main__':
    result = get_quotes(None)
    print(result)
