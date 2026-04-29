package com.mabinogion.bacnet;

import com.fasterxml.jackson.databind.ObjectMapper;
import com.serotonin.bacnet4j.LocalDevice;
import com.serotonin.bacnet4j.RemoteDevice;
import com.serotonin.bacnet4j.exception.BACnetException;
import com.serotonin.bacnet4j.npdu.ip.IpNetworkBuilder;
import com.serotonin.bacnet4j.npdu.ip.IpNetworkUtils;
import com.serotonin.bacnet4j.transport.DefaultTransport;
import com.serotonin.bacnet4j.type.Encodable;
import com.serotonin.bacnet4j.type.enumerated.ObjectType;
import com.serotonin.bacnet4j.type.enumerated.PropertyIdentifier;
import com.serotonin.bacnet4j.type.enumerated.Segmentation;
import com.serotonin.bacnet4j.type.primitive.CharacterString;
import com.serotonin.bacnet4j.type.primitive.ObjectIdentifier;
import com.serotonin.bacnet4j.type.primitive.Real;
import com.serotonin.bacnet4j.type.primitive.UnsignedInteger;
import com.serotonin.bacnet4j.util.RequestUtils;

import java.io.IOException;
import java.nio.file.Files;
import java.nio.file.Path;
import java.util.ArrayList;
import java.util.LinkedHashMap;
import java.util.List;
import java.util.Map;

public class PeerClient {
    private static final ObjectMapper OBJECT_MAPPER = new ObjectMapper();

    public static void main(String[] args) throws Exception {
        String sutAddr = requireEnv("MABI_BACNET_SUT_ADDR");
        int deviceInstance = Integer.parseInt(requireEnv("MABI_BACNET_DEVICE_INSTANCE"));
        int objectInstance = Integer.parseInt(requireEnv("MABI_BACNET_OBJECT_INSTANCE"));
        double writeValue = Double.parseDouble(requireEnv("MABI_BACNET_WRITE_VALUE"));
        Path transcriptPath = Path.of(requireEnv("MABI_BACNET_TRANSCRIPT_PATH"));
        Map<String, Object> transcript = new LinkedHashMap<>();
        transcript.put("peer", "bacnet4j");
        transcript.put("sut_addr", sutAddr);
        transcript.put("device_instance", deviceInstance);
        transcript.put("discovery_ok", false);
        transcript.put("read_ok", false);
        transcript.put("write_ok", false);
        transcript.put("property_multiple_ok", false);
        transcript.put("round_trip_value", 0.0d);
        List<String> errors = new ArrayList<>();
        transcript.put("errors", errors);

        LocalDevice localDevice = null;
        try {
            String[] hostPort = sutAddr.split(":");
            String host = hostPort[0];
            int port = Integer.parseInt(hostPort[1]);

            localDevice = new LocalDevice(
                    998004,
                    new DefaultTransport(
                            new IpNetworkBuilder()
                                    .withLocalBindAddress("127.0.0.1")
                                    .withBroadcast("127.255.255.255", 8)
                                    .withPort(47811)
                                    .build()
                    )
            );
            localDevice.initialize();

            RemoteDevice remoteDevice = new RemoteDevice(
                    localDevice,
                    deviceInstance,
                    IpNetworkUtils.toAddress(host, port)
            );
            remoteDevice.setDeviceProperty(PropertyIdentifier.maxApduLengthAccepted, new UnsignedInteger(512));
            remoteDevice.setDeviceProperty(PropertyIdentifier.segmentationSupported, Segmentation.noSegmentation);

            ObjectIdentifier objectId = new ObjectIdentifier(ObjectType.analogOutput, objectInstance);

            Real initial = RequestUtils.getProperty(localDevice, remoteDevice, objectId, PropertyIdentifier.presentValue);
            transcript.put("read_ok", initial != null);

            Map<PropertyIdentifier, Encodable> multi = RequestUtils.getProperties(
                    localDevice,
                    remoteDevice,
                    objectId,
                    null,
                    PropertyIdentifier.presentValue,
                    PropertyIdentifier.objectName
            );
            CharacterString objectName = (CharacterString) multi.get(PropertyIdentifier.objectName);
            Encodable multiplePresentValue = multi.get(PropertyIdentifier.presentValue);
            transcript.put(
                    "property_multiple_ok",
                    objectName != null && objectName.getValue() != null && multiplePresentValue != null
            );

            RequestUtils.writeProperty(
                    localDevice,
                    remoteDevice,
                    objectId,
                    PropertyIdentifier.presentValue,
                    new Real((float) writeValue),
                    8
            );
            transcript.put("write_ok", true);

            Real roundTrip = RequestUtils.getProperty(localDevice, remoteDevice, objectId, PropertyIdentifier.presentValue);
            if (roundTrip != null) {
                double observed = roundTrip.floatValue();
                transcript.put("round_trip_value", observed);
                if (Math.abs(observed - writeValue) > 0.01d) {
                    errors.add("BACnet4J round-trip drifted: expected " + writeValue + ", observed " + observed);
                }
            } else {
                errors.add("BACnet4J round-trip read returned null");
            }
        } catch (BACnetException | RuntimeException exc) {
            errors.add("BACnet4J peer failure: " + exc.getMessage());
        } finally {
            if (localDevice != null) {
                localDevice.terminate();
            }
            writeTranscript(transcriptPath, transcript);
        }

        if (!errors.isEmpty()) {
            System.exit(1);
        }
    }

    private static void writeTranscript(Path transcriptPath, Map<String, Object> transcript) throws IOException {
        Files.createDirectories(transcriptPath.getParent());
        OBJECT_MAPPER.writerWithDefaultPrettyPrinter().writeValue(transcriptPath.toFile(), transcript);
    }

    private static String requireEnv(String name) {
        String value = System.getenv(name);
        if (value == null || value.isBlank()) {
            throw new IllegalStateException("missing required environment variable: " + name);
        }
        return value;
    }
}
