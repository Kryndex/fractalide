extern crate capnp;
#[allow(unused_imports)]
use std::io::Read;

use result;
use result::Result;

use std::collections::HashMap;
use std::mem;

use std::sync::mpsc::{Sender, Receiver, SyncSender};
use std::sync::mpsc::sync_channel;

use scheduler::CompMsg;

pub struct IP {
    pub vec: Vec<u8>,
    pub action: String,
    reader: Option<capnp::message::Reader<capnp::serialize::OwnedSegments>>,
    builder: Option<capnp::message::Builder<capnp::message::HeapAllocator>>,
}

impl IP {
    pub fn new() -> Self {
        IP { vec: vec![],
             action: String::new(),
             reader: None,
             builder: None,
        }
    }

    pub fn get_root<'a, T: capnp::traits::FromPointerReader<'a>>(&'a mut self) -> Result<T> {
        let msg = try!(capnp::serialize::read_message(&mut &self.vec[..], capnp::message::ReaderOptions::new()));
        self.reader = Some(msg);
        Ok(try!(self.reader.as_ref().unwrap().get_root()))
    }
    pub fn init_root<'a, T: capnp::traits::FromPointerBuilder<'a>>(&'a mut self) -> T {
        let msg = capnp::message::Builder::new_default();
        self.builder = Some(msg);
        self.builder.as_mut().unwrap().init_root()
    }

    pub fn init_root_from_reader<'a, T: capnp::traits::FromPointerBuilder<'a>,
                                 U: capnp::traits::FromPointerReader<'a> + capnp::traits::SetPointerBuilder<T>>
        (&'a mut self) -> Result<T> {
        let reader = try!(capnp::serialize::read_message(&mut &self.vec[..], capnp::message::ReaderOptions::new()));
        self.reader = Some(reader);
        let reader: U = try!(self.reader.as_ref().unwrap().get_root());

        let mut msg = capnp::message::Builder::new_default();
        try!(msg.set_root(reader));
        self.builder = Some(msg);
        Ok(try!(self.builder.as_mut().unwrap().get_root()))
    }

    pub fn before_send(&mut self) -> Result<()> {
        let mut build = mem::replace(&mut self.builder, None);
        if let Some(ref mut b) = build {
            self.vec.clear();
            try!(capnp::serialize::write_message(&mut self.vec, b))
        }
        Ok(())

    }
}

impl Clone for IP {
    fn clone(&self) -> Self {
        IP {
            vec: self.vec.clone(),
            action: self.action.clone(),
            reader: None,
            builder: None,
        }
    }
}

#[derive(Clone)]
pub struct IPSender {
    pub sender: SyncSender<IP>,
    pub dest: String,
    pub sched: Sender<CompMsg>,
}

impl IPSender {
    pub fn send(&self, mut ip: IP) -> Result<()> {
        try!(ip.before_send());
        try!(self.sender.send(ip));
        if self.dest != "" {
            try!(self.sched.send(CompMsg::Inc(self.dest.clone())));
        }
        Ok(())
    }
}

pub struct Ports {
    name: String,
    sched: Sender<CompMsg>,
    inputs: HashMap<String, Receiver<IP>>,
    inputs_array: HashMap< String, HashMap<String, Receiver<IP>>>,
    outputs: HashMap<String, Option<IPSender>>,
    outputs_array: HashMap<String, HashMap<String, Option<IPSender>>>,
    senders: HashMap<String, IPSender>,
}

impl Ports {
    pub fn new(name: String, sched: Sender<CompMsg>,
               n_input: Vec<String>, n_input_array: Vec<String>,
               n_output: Vec<String>, n_output_array: Vec<String>) -> Result<(Self, HashMap<String, IPSender>)> {
        let mut senders: HashMap<String, IPSender> = HashMap::new();
        let mut inputs = HashMap::new();
        for i in n_input {
            let (s, r) = sync_channel(25);
            let s = IPSender {
                sender: s,
                dest: if i != "acc" && i != "option" { name.clone() } else { "".into() },
                sched: sched.clone(),
            };
            senders.insert(i.clone(), s);
            inputs.insert(i, r);
        }
        let mut inputs_array = HashMap::new();
        for i in n_input_array { inputs_array.insert(i, HashMap::new()); }
        let mut outputs = HashMap::new();
        for i in n_output { outputs.insert(i, None); }
        let mut outputs_array = HashMap::new();
        for i in n_output_array { outputs_array.insert(i, HashMap::new()); }
        let ports = Ports {
            name: name,
            sched: sched,
            inputs: inputs,
            inputs_array: inputs_array,
            outputs: outputs,
            outputs_array: outputs_array,
            senders: senders.clone(),
        };

        Ok((ports, senders))
    }

    pub fn get_sender(&self, port_in: &str) -> Result<IPSender> {
        self.senders.get(port_in).ok_or(result::Error::PortNotFound)
            .map(|sender| {
                sender.clone()
            })
    }

    pub fn get_input_selections(&self, port_in: &'static str) -> Result<Vec<String>> {
        self.inputs_array.get(port_in).ok_or(result::Error::PortNotFound)
            .map(|port| {
                port.keys().cloned().collect()
            })
    }

    pub fn get_output_selections(&self, port_out: &'static str) -> Result<Vec<String>> {
        self.outputs_array.get(port_out).ok_or(result::Error::PortNotFound)
            .map(|port| {
                port.keys().cloned().collect()
            })
    }

    pub fn recv(&self, port_in: &str) -> Result<IP> {
        if let Some(ref mut port) = self.inputs.get(port_in) {
            // Received the IP
            let ip = try!(port.recv());
            if port_in != "acc" && port_in != "option" {
                try!(self.sched.send(CompMsg::Dec(self.name.clone())));
            }
            Ok(ip)
        } else {
            Err(result::Error::PortNotFound)
        }
    }

    pub fn try_recv(&self, port_in: &str) -> Result<IP> {
        if let Some(ref mut port) = self.inputs.get(port_in) {
            let ip = try!(port.try_recv());
            if port_in != "acc" && port_in != "option" {
                try!(self.sched.send(CompMsg::Dec(self.name.clone())));
            }
            Ok(ip)
        } else {
            Err(result::Error::PortNotFound)
        }
    }

    pub fn recv_array(&self, port_in: &str, selection_in: &str) -> Result<IP> {
        self.inputs_array.get(port_in).ok_or(result::Error::PortNotFound)
            .and_then(|port|{
                port.get(selection_in).ok_or(result::Error::SelectionNotFound)
                    .and_then(|recv| {
                        let ip = try!(recv.recv());
                        if port_in != "acc" && port_in != "option" {
                            try!(self.sched.send(CompMsg::Dec(self.name.clone())));
                        }
                        Ok(ip)
                    })
            })
    }

    pub fn try_recv_array(&self, port_in: &str, selection_in: &str) -> Result<IP> {
        self.inputs_array.get(port_in).ok_or(result::Error::PortNotFound)
            .and_then(|port|{
                port.get(selection_in).ok_or(result::Error::SelectionNotFound)
                    .and_then(|recv| {
                        let ip = try!(recv.try_recv());
                        if port_in != "acc" && port_in != "option" {
                            try!(self.sched.send(CompMsg::Dec(self.name.clone())));
                        }
                        Ok(ip)
                    })
            })
    }

    pub fn send(&self, port_out: &str, mut ip: IP) -> Result<()> {
        self.outputs.get(port_out).ok_or(result::Error::PortNotFound)
            .and_then(|port|{
                port.as_ref().ok_or(result::Error::OutputPortNotConnected)
                    .and_then(|sender| {
                        sender.send(ip)
                    })
            })
    }

    pub fn send_array(&self, port_out: &str, selection_out: &str, mut ip: IP) -> Result<()> {
        self.outputs_array.get(port_out).ok_or(result::Error::PortNotFound)
            .and_then(|port| {
                port.get(selection_out).ok_or(result::Error::SelectionNotFound)
                    .and_then(|sender| {
                        sender.as_ref().ok_or(result::Error::OutputPortNotConnected)
                            .and_then(|sender| {
                                sender.send(ip)
                            })
                    })
            })
    }

    pub fn send_action(&self, port_out: &'static str, ip: IP) -> Result<()> {
        if try!(self.get_output_selections(&port_out)).contains(&ip.action) {
            self.send_array(&port_out, &ip.action.clone(), ip)
        } else {
            self.send(&port_out, ip)
        }
    }

    pub fn connect(&mut self, port_out: String, sender: IPSender) -> Result<()> {
        if !self.outputs.contains_key(&port_out) {
            return Err(result::Error::PortNotFound);
        }
        self.outputs.insert(port_out, Some(sender));
        Ok(())
    }

    pub fn connect_array(&mut self, port_out: String, selection_out: String, sender: IPSender) -> Result<()> {
        if !self.outputs_array.contains_key(&port_out) {
            return Err(result::Error::PortNotFound);
        }
        self.outputs_array.get_mut(&port_out).ok_or(result::Error::PortNotFound)
            .and_then(|port| {
                if !port.contains_key(&selection_out) {
                    return Err(result::Error::SelectionNotFound);
                }
                port.insert(selection_out, Some(sender));
                Ok(())
            })
    }

    pub fn disconnect(&mut self, port_out: String) -> Result<Option<IPSender>> {
        if !self.outputs.contains_key(&port_out) {
            return Err(result::Error::PortNotFound);
        }
        let old = self.outputs.insert(port_out, None);
        match old {
            Some(Some(ip_sender)) => {
                Ok(Some(ip_sender))
            }
            _ => { Ok(None) },
        }
    }

    pub fn disconnect_array(&mut self, port_out: String, selection_out: String) -> Result<Option<IPSender>> {
        if !self.outputs_array.contains_key(&port_out) {
            return Err(result::Error::PortNotFound);
        }
        self.outputs_array.get_mut(&port_out).ok_or(result::Error::PortNotFound)
            .and_then(|port| {
                if !port.contains_key(&selection_out) {
                    return Err(result::Error::SelectionNotFound);
                }
                let old = port.insert(port_out, None);
                match old {
                    Some(Some(ip_sender)) => {
                        Ok(Some(ip_sender))
                    }
                    _ => { Ok(None) },
                }
            })
    }

    pub fn set_receiver(&mut self, port: String, recv: Receiver<IP>) {
        self.inputs.insert(port, recv);
    }

    pub fn remove_receiver(&mut self, port: &str) -> Result<Receiver<IP>> {
        self.inputs.remove(port).ok_or(result::Error::PortNotFound)
            .map(|recv| { recv })
    }

    pub fn remove_array_receiver(&mut self, port: &str, selection: &str) -> Result<Receiver<IP>> {
        self.inputs_array.get_mut(port).ok_or(result::Error::PortNotFound)
            .and_then(|port| {
                port.remove(selection).ok_or(result::Error::SelectionNotFound)
                    .map(|recv| { recv })
            })
    }

    pub fn add_input_selection(&mut self, port_in: &str, selection_in: String) -> Result<IPSender> {
        let (s, r) = sync_channel(25);
        let s = IPSender {
            sender: s,
            dest: self.name.clone(),
            sched: self.sched.clone(),
        };
        self.inputs_array.get_mut(port_in)
            .ok_or(result::Error::PortNotFound)
            .map(|port| {
                port.insert(selection_in, r);
                s
            })
    }

    pub fn add_input_receiver(&mut self, port_in: &str, selection_in: String, r: Receiver<IP>) -> Result<()> {
        self.inputs_array.get_mut(port_in)
            .ok_or(result::Error::PortNotFound)
            .map(|port| {
                port.insert(selection_in, r);
                ()
            })
    }

    pub fn add_output_selection(&mut self, port_out: &str, selection_out: String) -> Result<()> {
        self.outputs_array.get_mut(port_out)
            .ok_or(result::Error::PortNotFound)
            .map(|port| {
                if !port.contains_key(&selection_out) {
                    port.insert(selection_out, None);
                }
                ()
            })
    }
}

#[allow(unused_imports)]
mod test_port {

    use super::Ports;

    use scheduler::CompMsg;

    use std::sync::mpsc::channel;
    #[test]
    fn ports() {
        assert!(1==1);
        let (s, r) = channel();


        let (mut p1, mut senders) = Ports::new("unique".into(), s,
                                               vec!["in".into(), "vec".into()],
                                               vec!["in_a".into()],
                                               vec!["out".into()],
                                               vec!["out_a".into()]
                                               ).expect("cannot create");
        assert!(senders.len() == 2);

        let s_in = senders.get("in").unwrap();

        p1.connect("out".into(), s_in.clone()).expect("cannot connect");

        let wrong = p1.try_recv("in");
        assert!(wrong.is_err());

        let ip = super::IP{ vec: vec![] };

        p1.send("out", ip).expect("cannot send");

        let ok = p1.try_recv("in");
        assert!(ok.is_ok());

        let ip = super::IP{ vec: vec![] };
        p1.send("out", ip).expect("cannot send second times");

        let nip = p1.recv("in");
        assert!(nip.is_ok());
        // test array ports

        let s_in = p1.add_input_selection("in_a", "1".into()).expect("cannot add input selection");

        p1.add_output_selection("out_a".into(), "a".into());
        p1.connect_array("out_a".into(), "a".into(), s_in).expect("cannot connect array");

        let ip = super::IP{ vec: vec![] };
        p1.send_array("out_a", "a", ip).expect("cannot send array");

        let nip = p1.recv_array("in_a", "1");
        assert!(nip.is_ok());

        let i = r.recv().expect("cannot received the sched");
        assert!(
            if let CompMsg::Inc(ref name) = i { name == "unique" } else { false }
            );
        let i = r.recv().expect("cannot received the sched");
        assert!(
            if let CompMsg::Dec(ref name) = i { name == "unique" } else { false }
            );
        let i = r.recv().expect("cannot received the sched");
        assert!(
            if let CompMsg::Inc(ref name) = i { name == "unique" } else { false }
            );
        let i = r.recv().expect("cannot received the sched");
        assert!(
            if let CompMsg::Dec(ref name) = i { name == "unique" } else { false }
            );
        let i = r.recv().expect("cannot received the sched");
        assert!(
            if let CompMsg::Inc(ref name) = i { name == "unique" } else { false }
            );
        let i = r.recv().expect("cannot received the sched");
        assert!(
            if let CompMsg::Dec(ref name) = i { name == "unique" } else { false }
            );
        let i = r.try_recv();
        assert!(i.is_err());
    }
}
