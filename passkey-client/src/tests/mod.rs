use super::*;
use coset::iana;
use passkey_authenticator::{MemoryStore, MockUserValidationMethod, UserCheck};
use passkey_types::{
    ctap2, encoding::try_from_base64url, rand::random_vec, webauthn::CollectedClientData, Bytes,
};
use serde::Deserialize;
use url::{ParseError, Url};

fn good_credential_creation_options() -> webauthn::PublicKeyCredentialCreationOptions {
    webauthn::PublicKeyCredentialCreationOptions {
        rp: webauthn::PublicKeyCredentialRpEntity {
            id: Some("future.1password.com".into()),
            name: "future.1password.com".into(),
        },
        user: webauthn::PublicKeyCredentialUserEntity {
            id: random_vec(16).into(),
            display_name: "wendy".into(),
            name: "wendy".into(),
        },
        challenge: random_vec(32).into(),
        pub_key_cred_params: vec![webauthn::PublicKeyCredentialParameters {
            ty: webauthn::PublicKeyCredentialType::PublicKey,
            alg: iana::Algorithm::ES256,
        }],
        timeout: None,
        exclude_credentials: Default::default(),
        authenticator_selection: Default::default(),
        hints: Some(vec![webauthn::PublicKeyCredentialHints::ClientDevice]),
        attestation: Default::default(),
        attestation_formats: Default::default(),
        extensions: Default::default(),
    }
}

fn good_credential_request_options(
    credential_id: impl Into<Bytes>,
) -> webauthn::PublicKeyCredentialRequestOptions {
    webauthn::PublicKeyCredentialRequestOptions {
        challenge: random_vec(32).into(),
        timeout: None,
        rp_id: Some("future.1password.com".into()),
        allow_credentials: Some(vec![webauthn::PublicKeyCredentialDescriptor {
            ty: webauthn::PublicKeyCredentialType::PublicKey,
            id: credential_id.into(),
            transports: None,
        }]),
        user_verification: Default::default(),
        hints: Some(vec![webauthn::PublicKeyCredentialHints::ClientDevice]),
        attestation: Default::default(),
        attestation_formats: Default::default(),
        extensions: Default::default(),
    }
}

fn uv_mock_with_creation(times: usize) -> MockUserValidationMethod {
    let mut user_mock = MockUserValidationMethod::new();
    user_mock
        .expect_is_verification_enabled()
        .returning(|| Some(true));
    user_mock
        .expect_check_user()
        .with(
            mockall::predicate::always(),
            mockall::predicate::eq(true),
            mockall::predicate::eq(true),
        )
        .returning(|_, _, _| {
            Ok(UserCheck {
                presence: true,
                verification: true,
            })
        })
        .times(times);
    user_mock.expect_is_presence_enabled().returning(|| true);
    user_mock
}

#[tokio::test]
async fn create_and_authenticate() {
    let auth = Authenticator::new(
        ctap2::Aaguid::new_empty(),
        MemoryStore::new(),
        uv_mock_with_creation(2),
    );
    let mut client = Client::new(auth);

    let origin = Url::parse("https://future.1password.com").unwrap();
    let options = webauthn::CredentialCreationOptions {
        public_key: good_credential_creation_options(),
    };
    let cred = client
        .register(&origin, options, DefaultClientData)
        .await
        .expect("failed to register with options");

    let credential_id = cred.raw_id;

    let auth_options = webauthn::CredentialRequestOptions {
        public_key: good_credential_request_options(credential_id),
    };
    client
        .authenticate(&origin, auth_options, DefaultClientData)
        .await
        .expect("failed to authenticate with freshly created credential");
}

#[tokio::test]
async fn create_and_authenticate_with_extra_client_data() {
    #[derive(Clone, Serialize, Deserialize)]
    struct AndroidClientData {
        android_package_name: String,
    }
    let auth = Authenticator::new(
        ctap2::Aaguid::new_empty(),
        MemoryStore::new(),
        uv_mock_with_creation(2),
    );
    let mut client = Client::new(auth);

    let origin = Url::parse("https://future.1password.com").unwrap();
    let options = webauthn::CredentialCreationOptions {
        public_key: good_credential_creation_options(),
    };
    let extra_data = AndroidClientData {
        android_package_name: "com.example.app".to_owned(),
    };
    let cred = client
        .register(
            &origin,
            options,
            DefaultClientDataWithExtra(extra_data.clone()),
        )
        .await
        .expect("failed to register with options");

    let returned_base64url_client_data_json: String = cred.response.client_data_json.into();
    let returned_client_data_json =
        try_from_base64url(returned_base64url_client_data_json.as_str())
            .expect("could not base64url decode client data");
    let returned_client_data: CollectedClientData<AndroidClientData> =
        serde_json::from_slice(&returned_client_data_json)
            .expect("could not json deserialize client data");
    assert_eq!(
        returned_client_data.extra_data.android_package_name,
        "com.example.app"
    );

    let credential_id = cred.raw_id;

    let auth_options = webauthn::CredentialRequestOptions {
        public_key: good_credential_request_options(credential_id),
    };
    let result = client
        .authenticate(
            &origin,
            auth_options,
            DefaultClientDataWithExtra(extra_data.clone()),
        )
        .await
        .expect("failed to authenticate with freshly created credential");

    let returned_base64url_client_data_json: String = result.response.client_data_json.into();
    let returned_client_data_json =
        try_from_base64url(returned_base64url_client_data_json.as_str())
            .expect("could not base64url decode client data");
    let returned_client_data: CollectedClientData<AndroidClientData> =
        serde_json::from_slice(&returned_client_data_json)
            .expect("could not json deserialize client data");
    assert_eq!(
        returned_client_data.extra_data.android_package_name,
        "com.example.app"
    );
}

#[tokio::test]
async fn create_and_authenticate_with_origin_subdomain() {
    let auth = Authenticator::new(
        ctap2::Aaguid::new_empty(),
        MemoryStore::new(),
        uv_mock_with_creation(2),
    );
    let mut client = Client::new(auth);

    let origin = Url::parse("https://www.future.1password.com").unwrap();
    let options = webauthn::CredentialCreationOptions {
        public_key: good_credential_creation_options(),
    };
    let cred = client
        .register(&origin, options, DefaultClientData)
        .await
        .expect("failed to register with options");

    let att_obj: ctap2::make_credential::Response =
        ciborium::de::from_reader(cred.response.attestation_object.as_slice())
            .expect("could not deserialize response");
    assert_eq!(
        att_obj.auth_data.rp_id_hash(),
        &sha256(b"future.1password.com")
    );

    let auth_options = webauthn::CredentialRequestOptions {
        public_key: good_credential_request_options(cred.raw_id),
    };
    let res = client
        .authenticate(&origin, auth_options, DefaultClientData)
        .await
        .expect("failed to authenticate with freshly created credential");
    let att_obj = ctap2::AuthenticatorData::from_slice(&res.response.authenticator_data)
        .expect("could not deserialize response");
    assert_eq!(att_obj.rp_id_hash(), &sha256(b"future.1password.com"));
}

#[tokio::test]
async fn create_and_authenticate_without_rp_id() {
    let auth = Authenticator::new(
        ctap2::Aaguid::new_empty(),
        MemoryStore::new(),
        uv_mock_with_creation(2),
    );
    let mut client = Client::new(auth);

    let origin = Url::parse("https://www.future.1password.com").unwrap();
    let options = webauthn::CredentialCreationOptions {
        public_key: webauthn::PublicKeyCredentialCreationOptions {
            rp: webauthn::PublicKeyCredentialRpEntity {
                id: None,
                name: "future.1password.com".into(),
            },
            ..good_credential_creation_options()
        },
    };
    let cred = client
        .register(&origin, options, DefaultClientData)
        .await
        .expect("failed to register with options");

    let att_obj: ctap2::make_credential::Response =
        ciborium::de::from_reader(cred.response.attestation_object.as_slice())
            .expect("could not deserialize response");
    assert_eq!(
        att_obj.auth_data.rp_id_hash(),
        &sha256(b"www.future.1password.com")
    );

    let auth_options = webauthn::CredentialRequestOptions {
        public_key: webauthn::PublicKeyCredentialRequestOptions {
            rp_id: None,
            ..good_credential_request_options(cred.raw_id)
        },
    };
    let res = client
        .authenticate(&origin, auth_options, DefaultClientData)
        .await
        .expect("failed to authenticate with freshly created credential");
    let att_obj = ctap2::AuthenticatorData::from_slice(&res.response.authenticator_data)
        .expect("could not deserialize response");
    assert_eq!(att_obj.rp_id_hash(), &sha256(b"www.future.1password.com"));
}

#[tokio::test]
async fn create_and_authenticate_without_cred_params() {
    let auth = Authenticator::new(
        ctap2::Aaguid::new_empty(),
        MemoryStore::new(),
        uv_mock_with_creation(2),
    );
    let mut client = Client::new(auth);

    let origin = Url::parse("https://future.1password.com").unwrap();
    let options = webauthn::CredentialCreationOptions {
        public_key: webauthn::PublicKeyCredentialCreationOptions {
            pub_key_cred_params: Vec::new(),
            ..good_credential_creation_options()
        },
    };
    let cred = client
        .register(&origin, options, DefaultClientData)
        .await
        .expect("failed to register with options");

    let credential_id = cred.raw_id;

    let auth_options = webauthn::CredentialRequestOptions {
        public_key: good_credential_request_options(credential_id),
    };
    client
        .authenticate(&origin, auth_options, DefaultClientData)
        .await
        .expect("failed to authenticate with freshly created credential");
}

#[test]
fn validate_rp_id() -> Result<(), ParseError> {
    let client = RpIdVerifier::new(public_suffix::DEFAULT_PROVIDER);

    let example = Url::parse("https://example.com")?.into();
    let com_tld = client.assert_domain(&example, Some("com"));
    assert_eq!(com_tld, Err(WebauthnError::InvalidRpId));

    let example_dots = Url::parse("https://example...com")?.into();
    let bunch_of_dots = client.assert_domain(&example_dots, Some("...com"));
    assert_eq!(bunch_of_dots, Err(WebauthnError::InvalidRpId));

    let future = Url::parse("https://www.future.1password.com")?.into();
    let sub_domain_ignored = client.assert_domain(&future, Some("future.1password.com"));
    assert_eq!(sub_domain_ignored, Ok("future.1password.com"));

    let use_effective_domain = client.assert_domain(&future, None);
    assert_eq!(use_effective_domain, Ok("www.future.1password.com"));

    let not_protected = Url::parse("http://example.com")?.into();
    let not_https = client.assert_domain(&not_protected, Some("example.com"));
    assert_eq!(not_https, Err(WebauthnError::UnprotectedOrigin));

    let localhost = Url::parse("http://localhost:8080")?.into();
    let should_still_match = client.assert_domain(&localhost, Some("example.com"));
    assert_eq!(should_still_match, Err(WebauthnError::OriginRpMissmatch));

    let localhost_not_allowed = client.assert_domain(&localhost, Some("localhost"));
    assert_eq!(
        localhost_not_allowed,
        Err(WebauthnError::InsecureLocalhostNotAllowed)
    );
    let localhost_not_allowed = client.assert_domain(&localhost, None);
    assert_eq!(
        localhost_not_allowed,
        Err(WebauthnError::InsecureLocalhostNotAllowed)
    );

    let client = client.allows_insecure_localhost(true);
    let skips_http_and_tld_check = client.assert_domain(&localhost, Some("localhost"));
    assert_eq!(skips_http_and_tld_check, Ok("localhost"));
    let skips_http_and_tld_check = client.assert_domain(&localhost, None);
    assert_eq!(skips_http_and_tld_check, Ok("localhost"));

    Ok(())
}

struct BrokenTLDProvider {}
impl public_suffix::EffectiveTLDProvider for BrokenTLDProvider {
    // Notice that this just returns Err() for every domain regardless.
    // This is only done to allow the test's assertion to prove that we
    // are actually using this verifier and not the default one.
    fn effective_tld_plus_one<'a>(
        &self,
        _domain: &'a str,
    ) -> Result<&'a str, public_suffix::Error> {
        Err(public_suffix::Error::CannotDeriveETldPlus1)
    }
}
#[test]
fn validate_domain_with_private_list_provider() -> Result<(), ParseError> {
    let my_custom_provider = BrokenTLDProvider {};
    let client = RpIdVerifier::new(my_custom_provider);

    // Notice that, in this test, this is a legitimate origin/rp_id combination
    // We assert that this produces an error to prove that we are indeed using our
    // BrokenTLDProvider which always returns Err() regardless of the TLD.
    let origin = Url::parse("https://www.future.1password.com")?.into();
    let rp_id = "future.1password.com";
    let result = client.assert_domain(&origin, Some(rp_id));
    assert_eq!(result, Err(WebauthnError::InvalidRpId));

    Ok(())
}

fn user_mock_with_uv() -> MockUserValidationMethod {
    let mut user_mock = MockUserValidationMethod::new();
    user_mock
        .expect_is_verification_enabled()
        .returning(|| Some(true));
    user_mock.expect_check_user().returning(|_, _, _| {
        Ok(UserCheck {
            presence: true,
            verification: true,
        })
    });
    // Always called by `get_info`
    user_mock
        .expect_is_verification_enabled()
        .returning(|| Some(true));
    user_mock.expect_is_presence_enabled().returning(|| true);
    user_mock
}

fn user_mock_without_uv() -> MockUserValidationMethod {
    let mut user_mock = MockUserValidationMethod::new();
    user_mock.expect_check_user().returning(|_, _, _| {
        Ok(UserCheck {
            presence: true,
            verification: false,
        })
    });
    // Always called by `get_info`
    user_mock
        .expect_is_verification_enabled()
        .returning(|| Some(true));
    user_mock.expect_is_presence_enabled().returning(|| true);
    user_mock
}

#[tokio::test]
async fn client_register_triggers_uv_when_uv_is_required() {
    // Arrange
    let auth = Authenticator::new(
        ctap2::Aaguid::new_empty(),
        MemoryStore::new(),
        user_mock_with_uv(),
    );
    let mut client = Client::new(auth);
    let origin = Url::parse("https://future.1password.com").unwrap();
    let mut options = webauthn::CredentialCreationOptions {
        public_key: good_credential_creation_options(),
    };
    options.public_key.authenticator_selection = Some(AuthenticatorSelectionCriteria {
        user_verification: UserVerificationRequirement::Required,
        authenticator_attachment: Default::default(),
        resident_key: Default::default(),
        require_resident_key: Default::default(),
    });

    // Act & Assert
    client
        .register(&origin, options, DefaultClientData)
        .await
        .expect("failed to register with options");
}

#[tokio::test]
async fn client_register_does_not_trigger_uv_when_uv_is_discouraged() {
    // Arrange
    let auth = Authenticator::new(
        ctap2::Aaguid::new_empty(),
        MemoryStore::new(),
        user_mock_without_uv(),
    );
    let mut client = Client::new(auth);
    let origin = Url::parse("https://future.1password.com").unwrap();
    let mut options = webauthn::CredentialCreationOptions {
        public_key: good_credential_creation_options(),
    };
    options.public_key.authenticator_selection = Some(AuthenticatorSelectionCriteria {
        user_verification: UserVerificationRequirement::Discouraged,
        authenticator_attachment: Default::default(),
        resident_key: Default::default(),
        require_resident_key: Default::default(),
    });

    // Act & Assert
    client
        .register(&origin, options, DefaultClientData)
        .await
        .expect("failed to register with options");
}
