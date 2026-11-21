use crate::sys::{SysAllocString, SysAllocStringLen, BSTR, E_INVALIDARG, E_OUTOFMEMORY, HRESULT, S_OK};

pub const SUCCESS_TEXT: &str = "The operation completed successfully";
pub const UNRECOGNIZED_TEXT: &str = "Unrecognized error";

pub static ERROR_TABLE: &[(u32, &str, Option<&str>)] = &[
    (0x8C1101F8, "The remote extension encountered an error", None),
    (0x8C1101F9, "The remote agent encountered an error", None),
    (0x8C1101FA, "The remote extension was redirected", None),
    (0x8C1101FB, "The specified operation was not found", Some("The required service on the console is not available.  Verify\r\nthat your console has been setup as a devkit and provisioned.\r\n\r\nRefer to the Microsoft Game Development Kit documentation for more information.\r\n")),
    (0x8C1101FC, "The specified session was not found", None),
    (0x8C1101FD, "The specified extension was not found", Some("The required service on the console is not available.  Verify\r\nthat your console has been setup as a devkit and provisioned.\r\n\r\nRefer to the Microsoft Game Development Kit documentation for more information.\r\n")),
    (0x8C1101FE, "The remote agent is prohibited", None),
    (0x8C1101FF, "Protocol mismatch between PC and devkit", Some("There is a version mismatch between the server software and this application.\r\nYou will need to update your client and/or server to ensure that both are up to date.\r\n\r\nRefer to the Microsoft Game Development Kit documentation for more information.\r\n")),
    (0x8C110201, "Unexpected one way message received", Some("An unexpected error occurred while processing the command.  Please retry\r\nthe operation.  If errors persist, reboot the console.\r\nIf the issue continues after a console reboot, the PC tools and console\r\nversion may be incompatible.\r\n")),
    (0x8C110202, "Unexpected begin message received", Some("An unexpected error occurred while processing the command.  Please retry\r\nthe operation.  If errors persist, reboot the console.\r\nIf the issue continues after a console reboot, the PC tools and console\r\nversion may be incompatible.\r\n")),
    (0x8C110203, "Unexpected more message received", Some("An unexpected error occurred while processing the command.  Please retry\r\nthe operation.  If errors persist, reboot the console.\r\nIf the issue continues after a console reboot, the PC tools and console\r\nversion may be incompatible.\r\n")),
    (0x8C110204, "Unexpected end message received", Some("An unexpected error occurred while processing the command.  Please retry\r\nthe operation.  If errors persist, reboot the console.\r\nIf the issue continues after a console reboot, the PC tools and console\r\nversion may be incompatible.\r\n")),
    (0x8C1102FC, "Unexpected close message received", Some("An unexpected error occurred while processing the command.  Please retry\r\nthe operation.  If errors persist, reboot the console.\r\nIf the issue continues after a console reboot, the PC tools and console\r\nversion may be incompatible.\r\n")),
    (0x8C1102FD, "Unexpected session response message received", Some("An unexpected error occurred while processing the command.  Please retry\r\nthe operation.  If errors persist, reboot the console.\r\nIf the issue continues after a console reboot, the PC tools and console\r\nversion may be incompatible.\r\n")),
    (0x8C1102FE, "Unexpected session request message received", Some("An unexpected error occurred while processing the command.  Please retry\r\nthe operation.  If errors persist, reboot the console.\r\nIf the issue continues after a console reboot, the PC tools and console\r\nversion may be incompatible.\r\n")),
    (0x8C1102FF, "Unexpected disconnect message received", Some("An unexpected error occurred while processing the command.  Please retry\r\nthe operation.  If errors persist, reboot the console.\r\nIf the issue continues after a console reboot, the PC tools and console\r\nversion may be incompatible.\r\n")),
    (0x8C110301, "An unexpected send error occurred", None),
    (0x8C110302, "An unexpected send error occurred", None),
    (0x8C110303, "Failed to connect to the specified devkit", Some("No connection could be made to the target console.\r\nVerify the specified HostName or IP address. Check firewall, \r\nnetwork settings, and ability to ping the console.\r\n\r\nRefer to \"Configuring development network access\" in the Microsoft\r\nGDK documentation (http://aka.ms/GDKNetworkAccess)\r\n")),
    (0x8C110304, "Failed to open the specified port", Some("The Xbox tool server does not appear to be running on the Xbox console.\r\nVerify that it is operational, and then try again.\r\n\r\nRefer to the Microsoft Game Development Kit documentation for more information.\r\n")),
    (0x8C110305, "An unexpected transport error occurred", None),
    (0x8C110306, "An unexpected transport error occurred", None),
    (0x8C110307, "An unexpected transport error occurred", None),
    (0x8C110308, "An unexpected transport error occurred", None),
    (0x8C110309, "An unexpected transport error occurred", None),
    (0x8C11030A, "Connection closed", None),
    (0x8C11030B, "A message was not completely received before timeout", Some("The request to the Xbox console has timed out. Please verify your\r\nconnection to the server, and then try again.\r\n\r\nRefer to the Microsoft Game Development Kit documentation for more information.")),
    (0x8C11030C, "Failed to connect to the specified devkit", Some("The device responded to a ping request and is present on the network.\r\nCheck firewall to ensure tools ports are allowed and confirm the console\r\nis configured properly as a devkit. If this issue persists, attempt a\r\nreboot of the console.\r\n\r\nRefer to \"Configuring development network access\" in the Microsoft\r\nGDK documentation (http://aka.ms/GDKNetworkAccess)\r\n")),
    (0x8C110401, "An unexpected message was received", None),
    (0x8C110402, "An unexpected message was received", None),
    (0x8C110403, "The session with the console was closed", None),
    (0x8C110404, "The session with the console was closed", None),
    (0x8C110405, "The session with the console was closed", None),
    (0x8C110407, "No devkit address supplied", Some("Run XBCONNECT or use the /X switch to specify a console address.")),
    (0x8C110408, "The VM target in the devkit address is not valid", None),
    (0x8C11040B, "Access to this console is limited and requires \r\nspecifying the correct key to access", Some("You must supply the correct access key for this console.\r\nYou can reset the access key in the Developer Settings page on the console.\r\n\r\nRefer to the Microsoft Game Development Kit documentation for more information.\r\n")),
    (0x8C11040D, "Game OS is not running on target console", Some("Start a game and retry the command.")),
    (0x8C11040E, "The console is not yet ready to process requests", Some("Please wait for the console to finish rebooting.")),
    (0x8C11040F, "A key was supplied but isn't required by the console", Some("Please xbconnect to the console without specifying a key.")),
    (0x8C110410, "An operation timed out waiting for a response from the console", Some("Retry the operation.")),
    (0x8C110411, "The console is taking a system update", Some("Please wait for the console to finish the update.")),
    (0x8C110412, "The console did not successfully boot", Some("Please attempt a reboot via xbreboot or physically reboot the console.")),
    (0x8C110500, "XBTPLink was unable to determine the address of the peer socket", Some("Retry the operation.")),
    (0x8C110501, "XBTPLink did not immediately receive a connect response with a \r\nwait time of 0", Some("Ensure XBTPLink is configured for the target and retry the operation.")),
    (0x8C110502, "XBTPLink received invalid data for a connect response", Some("Retry the operation.")),
    (0x8C110504, "An error occurred while waiting on an event for an incoming \r\nXBTPLink connection", Some("Retry the operation.")),
    (0x8C110505, "XBTPLink received invalid accept data in a connect request packet", Some("Retry the operation.")),
    (0x8C110506, "XBTPLink failed to send connect packet response to remote host", Some("Retry the operation.")),
    (0x8C110507, "XBTPLink cannot determine the XBTPLink handle type", Some("Retry the operation.")),
    (0x8C110508, "XBTPLink initialization previously failed, calls cannot be made to \r\nXBTPLink APIs", Some("Ensure the XbtpLinkSvc.exe is enabled, retry the operation,\r\nrestart XbtpLinkSvc.exe.")),
    (0x8C110509, "XBTPLink I/O Completion Port creation failed", Some("Retry the operation.")),
    (0x8C11050A, "XBTPLink worker thread creation failed", Some("Retry the operation, if this condition persists check memory usage \r\nof the computer.")),
    (0x8C11050B, "The provided XBTPLink handle is already connected or listening", Some("Retry the operation.")),
    (0x8C11050C, "XBTPLink channel initialization failed, unable to open device object", Some("Ensure the XbtpLinkSvc.exe is enabled, retry the operation,\r\nrestart XbtpLinkSvc.exe.")),
    (0x8C11050D, "XBTPLink channel initialization failed when binding the I/O Completion Port \r\nfor the handle", Some("Retry the operation.")),
    (0x8C11050F, "XBTPLink channel initialization failed while attempting to send the initial \r\nconnect packet", Some("Retry the operation.")),
    (0x8C110510, "XBTPLink channel initialization failed when attempting to receive the \r\nconnect packet response", Some("Retry the operation.")),
    (0x8C110511, "The XBTPLink service failed to start", Some("Ensure XbtpLinkSvc.exe is enabled, software dependencies are located next \r\nto the executable, and it is able to execute.  Retry the operation.")),
    (0x8C110512, "Initialization of the XBTPLink routing engine failed", Some("Ensure XbtpLinkSvc.exe is enabled, software dependencies are located next \r\nto the executable, and it is able to execute.  Retry the operation.")),
    (0x8C110514, "A packet payload overflow was encountered", Some("This is an internal error. Retry the operation.")),
    (0x8C110515, "Local XBTPLink routing service disabled", Some("Ensure XbtpLinkSvc.exe is enabled: XbtpLinkSvc.exe /enable:service")),
    (0x8C110516, "Cannot connect to remote host", Some("Ensure a network path is accessible to the remote host and retry the operation.")),
    (0x8C110517, "Remote host route disconnected", Some("Ensure a network path is accessible to the remote host and retry the \r\noperation. Ensure XbtpLinkSvc.exe is running.")),
    (0x8C110518, "Remote host closed connection", Some("The connection was closed by the remote host. This usually indicates the\r\nremote process terminated unexpectedly or deliberately closed the connection\r\nbefore the local process received all expected data. Retry the operation or\r\nreboot the console if the problem persists.")),
    (0x8C110519, "Attempt to send data after shutdown", Some("Indicates an internal programming error for XBTPLink API usage.")),
    (0x8C11051A, "No target port listening on host", Some("No application is listening on the XBTPLink virtual port provided. Retry\r\nthe operation, verify the target console is in the correct state, or reboot\r\nthe console if the problem persists.")),
    (0x8C11051B, "Packet header was corrupted", Some("This is an internal error. Retry the operation.")),
    (0x8C11051C, "An unknown XBTPLink error occurred", Some("This is an internal error. Retry the operation.")),
    (0x8C11051D, "XBTPLink was passed a bad handle", Some("Indicates an internal programming error for XBTPLink API usage. Retry the operation.")),
];

pub fn lookup(hr: HRESULT) -> (&'static str, Option<&'static str>) {
    if hr >= 0 {
        return (SUCCESS_TEXT, None);
    }
    match ERROR_TABLE.iter().find(|e| e.0 == hr as u32) {
        Some(e) => (e.1, e.2),
        None => (UNRECOGNIZED_TEXT, None),
    }
}

pub fn transport_error_text(hr: HRESULT) -> (String, Option<String>) {
    let (text, action) = lookup(hr);
    (format!("{}. (0x{:08X})", text, hr as u32), action.filter(|a| !a.is_empty()).map(|a| a.to_string()))
}

#[no_mangle]
pub unsafe extern "C" fn XbtpGetTransportErrorText(hr: HRESULT, text: *mut BSTR, action: *mut BSTR) -> HRESULT {
    if text.is_null() {
        return E_INVALIDARG;
    }
    *text = core::ptr::null_mut();
    if !action.is_null() {
        *action = core::ptr::null_mut();
    }
    let (t, a) = transport_error_text(hr);
    let w: Vec<u16> = t.encode_utf16().collect();
    let b = SysAllocStringLen(w.as_ptr(), w.len() as u32);
    if b.is_null() {
        return E_OUTOFMEMORY;
    }
    *text = b;
    if !action.is_null() {
        if let Some(a) = a {
            let wa: Vec<u16> = a.encode_utf16().chain(core::iter::once(0)).collect();
            let ab = SysAllocString(wa.as_ptr());
            if ab.is_null() {
                return E_OUTOFMEMORY;
            }
            *action = ab;
        }
    }
    S_OK
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sys::{from_wide, SysFreeString, SysStringLen};

    unsafe fn call(hr: HRESULT, want_action: bool) -> (HRESULT, Option<String>, Option<String>) {
        let mut t: BSTR = 1 as BSTR;
        let mut a: BSTR = 1 as BSTR;
        let r = XbtpGetTransportErrorText(hr, &mut t, if want_action { &mut a } else { core::ptr::null_mut() });
        let ts = if t.is_null() { None } else {
            let s = from_wide(t);
            assert_eq!(SysStringLen(t) as usize, s.encode_utf16().count());
            SysFreeString(t);
            Some(s)
        };
        let as_ = if !want_action || a.is_null() { None } else {
            let s = from_wide(a);
            SysFreeString(a);
            Some(s)
        };
        (r, ts, as_)
    }

    #[test]
    fn success_text() {
        let (r, t, a) = unsafe { call(0, true) };
        assert_eq!(r, S_OK);
        assert_eq!(t.unwrap(), "The operation completed successfully. (0x00000000)");
        assert!(a.is_none());
        let (_, t, _) = unsafe { call(1, true) };
        assert_eq!(t.unwrap(), "The operation completed successfully. (0x00000001)");
    }

    #[test]
    fn unknown_error_text() {
        let (r, t, a) = unsafe { call(0x80004005u32 as i32, true) };
        assert_eq!(r, S_OK);
        assert_eq!(t.unwrap(), "Unrecognized error. (0x80004005)");
        assert!(a.is_none());
    }

    #[test]
    fn table_entries_format() {
        let (r, t, _) = unsafe { call(0x8C1101F8u32 as i32, false) };
        assert_eq!(r, S_OK);
        assert_eq!(t.unwrap(), "The remote extension encountered an error. (0x8C1101F8)");
        for e in ERROR_TABLE {
            let (r, t, a) = unsafe { call(e.0 as i32, true) };
            assert_eq!(r, S_OK);
            assert_eq!(t.unwrap(), format!("{}. (0x{:08X})", e.1, e.0));
            assert_eq!(a.as_deref(), e.2.filter(|x| !x.is_empty()));
        }
    }

    #[test]
    fn table_is_sorted_unique() {
        assert_eq!(ERROR_TABLE.len(), 69);
        for w in ERROR_TABLE.windows(2) {
            assert!(w[0].0 < w[1].0);
        }
    }

    #[test]
    fn null_text_is_invalid() {
        let mut a: BSTR = 1 as BSTR;
        assert_eq!(unsafe { XbtpGetTransportErrorText(0, core::ptr::null_mut(), &mut a) }, E_INVALIDARG);
        assert_eq!(a, 1 as BSTR);
    }

    #[test]
    fn rust_helper_matches_export() {
        let (t, a) = transport_error_text(0x8C1101F9u32 as i32);
        assert_eq!(t, "The remote agent encountered an error. (0x8C1101F9)");
        assert!(a.is_none());
    }
}
