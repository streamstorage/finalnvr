const rtc_configuration = {iceServers: [{urls: "stun:stun.l.google.com:19302"},
                /* TODO: do not keep these static and in clear text in production,
                * and instead use one of the mechanisms discussed in
                * https://groups.google.com/forum/#!topic/discuss-webrtc/nn8b6UboqRA
                */
                {'urls': 'turn:turn.homeneural.net:3478?transport=udp',
                'credential': '1qaz2wsx',
                'username': 'test'
                }],
    /* Uncomment the following line to ensure the turn server is used
    * while testing. This should be kept commented out in production,
    * as non-relay ice candidates should be preferred
    */
    // iceTransportPolicy: "relay",
};

export class Webrtc {
    wsConn: WebSocket | undefined
    peerConnection: RTCPeerConnection | undefined
    setStatus: (val: string) => void
    peerId: string
    sessionId: string | undefined
    previewId: string
    
    constructor(wsUrl: string, f: (val: string) => void, peerId: string, previewId: string) {
        this.setStatus = f

        this.setStatus("Connecting to server " + wsUrl);
        this.wsConn = new WebSocket(wsUrl);
        /* When connected, immediately register with the server */
        this.wsConn.addEventListener('open', (_) => {
            this.setStatus("Connecting to the peer");
            this.connectPeer();
        });
        this.wsConn.addEventListener('message', this.onServerMessage.bind(this));
        this.wsConn.addEventListener('error', this.onServerError.bind(this));
        this.wsConn.addEventListener('close', this.onServerClose.bind(this));

        this.peerConnection = undefined
        this.peerId = peerId
        this.sessionId = undefined
        this.previewId = previewId

        // this.our_id = our_id;
        // this.closed_callback = closed_callback;
        // this.data_channel = null;
        // this.input = null;
    }

    close() {
        if (this.wsConn) {
            this.wsConn.close();
            this.wsConn = undefined;
        }
    }

    onServerError(_: any) {
        this.handleIncomingError('Server error');
    };

    onServerClose(_: any) {
        this.resetState();
    }

    handleIncomingError(_: any) {
        this.resetState();
    };

    resetState() {
        if (this.peerConnection) {
            this.peerConnection.close();
            this.peerConnection = undefined;
        }
        // TODO:
        // var videoElement = this.getVideoElement();
        // if (videoElement) {
        //     videoElement.pause();
        //     videoElement.src = "";
        // }

        if (this.wsConn) {
            this.wsConn.close();
            this.wsConn = undefined;
        }

        // this.input && this.input.detach();
        // this.data_channel = null;
    };

    onServerMessage(event: any) {
        console.log("Received " + event.data);
        var msg: any
        try {
            msg = JSON.parse(event.data);
        } catch (e) {
            if (e instanceof SyntaxError) {
                this.handleIncomingError("Error parsing incoming JSON: " + event.data);
            } else {
                this.handleIncomingError("Unknown error parsing response: " + event.data);
            }
            return;
        }

        if (msg.type == "sessionStarted") {
            this.setStatus("Registered with server");
            this.sessionId = msg.sessionId;
        } else if (msg.type == "error") {
            this.handleIncomingError(msg.details);
        } else if (msg.type == "endSession") {
            this.resetState();
        } else if (msg.type == "peer") {
            // Incoming peer message signals the beginning of a call
            if (this.peerConnection === undefined)
                this.createCall(msg);

            if (msg.sdp != null) {
                this.onIncomingSDP(msg.sdp);
            } else if (msg.ice != null) {
                this.onIncomingICE(msg.ice);
            } else {
                this.handleIncomingError("Unknown incoming JSON: " + msg);
            }
        }
    };

    connectPeer() {
        this.setStatus("Connecting " + this.peerId);
        this.wsConn?.send(JSON.stringify({
            "type": "startSession",
            "peerId": this.peerId
        }));
    };

    getVideoElement() {
        return document.getElementById(this.previewId) as HTMLVideoElement;
    };

    createCall(_: any) {
        console.log('Creating RTCPeerConnection');

        this.peerConnection = new RTCPeerConnection(rtc_configuration);
        this.peerConnection.ontrack = (event) => {
            var videoTracks = event.streams[0].getVideoTracks();
            var audioTracks = event.streams[0].getAudioTracks();
    
            console.log(videoTracks);
    
            if (videoTracks.length > 0) {
                console.log('Incoming stream: ' + videoTracks.length + ' video tracks and ' + audioTracks.length + ' audio tracks');
                this.getVideoElement().srcObject = event.streams[0];
                //this.getVideoElement().play();
                //desktop.value.srcObject = event.streams[0];
            } else {
                this.handleIncomingError('Stream with unknown tracks added, resetting');
            }
        };

        this.peerConnection.onicecandidate = (event) => {
            if (event.candidate == null) {
                console.log("ICE Candidate was null, done");
                return;
            }
            this.wsConn?.send(JSON.stringify({
                "type": "peer",
                "sessionId": this.sessionId,
                "ice": event.candidate.toJSON()
            }));
        };

        this.setStatus("Created peer connection for call, waiting for SDP");
    };

    setError(text: string) {
        console.error(text);
        this.resetState();
    };

    onRemoteDescriptionSet() {
        this.setStatus("Remote SDP set");
        this.setStatus("Got SDP offer");
        this.peerConnection?.createAnswer()
        .then(this.onLocalDescription.bind(this)).catch(this.setError);
    }

    onLocalDescription(desc: any) {
        console.log("Got local description: " + JSON.stringify(desc), this);
        var thiz = this;
        this.peerConnection?.setLocalDescription(desc).then(() => {
            this.setStatus("Sending SDP answer");
            var sdp = {
                'type': 'peer',
                'sessionId': this.sessionId,
                'sdp': this.peerConnection?.localDescription?.toJSON()
            };
            this.wsConn?.send(JSON.stringify(sdp));
        }).catch(function(e) {
            thiz.setError(e);
        });
    };

    // SDP offer received from peer, set remote description and create an answer
    onIncomingSDP(sdp: any) {
        var thiz = this;
        this.peerConnection?.setRemoteDescription(sdp)
            .then(this.onRemoteDescriptionSet.bind(this))
            .catch(function(e) {
                thiz.setError(e)
            });
    };

    // ICE candidate received from peer, add it to the peer connection
    onIncomingICE(ice: any) {
        var candidate = new RTCIceCandidate(ice);
        var thiz = this;
        this.peerConnection?.addIceCandidate(candidate).catch(function(e) {
            thiz.setError(e)
        });
    };
}
