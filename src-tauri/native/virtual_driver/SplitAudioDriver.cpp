#include <aspl/Driver.hpp>
#include <aspl/Device.hpp>
#include <aspl/Plugin.hpp>
#include <aspl/IORequestHandler.hpp>
#include <aspl/ControlRequestHandler.hpp>
#include <CoreAudio/AudioServerPlugIn.h>
#include <CoreFoundation/CoreFoundation.h>
#include <atomic>
#include <memory>
#include <string>
#include <vector>

// Per-device loopback ring buffer.
struct DeviceRing {
    static constexpr uint32_t kFrames = 16384;
    uint32_t channels;
    uint32_t nSamples;
    std::vector<float> buf;
    std::atomic<uint32_t> writeHead{0};

    explicit DeviceRing(uint32_t ch)
        : channels(ch), nSamples(kFrames * ch), buf(kFrames * ch, 0.0f) {}
};

class SplitIOHandler : public aspl::IORequestHandler,
                       public aspl::ControlRequestHandler {
    DeviceRing& ring_;
public:
    explicit SplitIOHandler(DeviceRing& r) : ring_(r) {}

    void OnWriteMixedOutput(
        const std::shared_ptr<aspl::Stream>&,
        Float64, Float64,
        const void* buff, UInt32 bytes) override
    {
        const float* src = static_cast<const float*>(buff);
        const uint32_t n = bytes / sizeof(float);
        uint32_t pos = ring_.writeHead.load(std::memory_order_relaxed);
        for (uint32_t i = 0; i < n; ++i) {
            ring_.buf[(pos + i) % ring_.nSamples] = src[i];
        }
        ring_.writeHead.store((pos + n) % ring_.nSamples, std::memory_order_release);
    }

    void OnReadClientInput(
        const std::shared_ptr<aspl::Client>&,
        const std::shared_ptr<aspl::Stream>&,
        Float64, Float64,
        void* buff, UInt32 bytes) override
    {
        float* dst = static_cast<float*>(buff);
        const uint32_t n = bytes / sizeof(float);
        const uint32_t w = ring_.writeHead.load(std::memory_order_acquire);
        const uint32_t r = (w + ring_.nSamples - n) % ring_.nSamples;
        for (uint32_t i = 0; i < n; ++i) {
            dst[i] = ring_.buf[(r + i) % ring_.nSamples];
        }
    }
};

// Config read from Contents/Resources/devices.plist at startup.
struct DeviceConfig { std::string id; std::string name; };

static std::string CFStr(CFStringRef s) {
    if (!s) return {};
    if (const char* c = CFStringGetCStringPtr(s, kCFStringEncodingUTF8)) return c;
    CFIndex len = CFStringGetLength(s) * 4 + 1;
    std::string out(len, '\0');
    CFStringGetCString(s, out.data(), len, kCFStringEncodingUTF8);
    out.resize(strlen(out.c_str()));
    return out;
}

static std::vector<DeviceConfig> ReadConfig() {
    CFBundleRef bundle = CFBundleGetBundleWithIdentifier(CFSTR("com.horuse.splitwave.audio"));
    if (!bundle) return {};

    CFURLRef url = CFBundleCopyResourceURL(bundle, CFSTR("devices"), CFSTR("plist"), nullptr);
    if (!url) return {};

    CFReadStreamRef stream = CFReadStreamCreateWithFile(nullptr, url);
    CFRelease(url);
    if (!stream || !CFReadStreamOpen(stream)) {
        if (stream) CFRelease(stream);
        return {};
    }

    CFErrorRef err = nullptr;
    CFPropertyListRef plist = CFPropertyListCreateWithStream(
        nullptr, stream, 0, kCFPropertyListImmutable, nullptr, &err);
    CFReadStreamClose(stream);
    CFRelease(stream);
    if (err) CFRelease(err);

    if (!plist || CFGetTypeID(plist) != CFArrayGetTypeID()) {
        if (plist) CFRelease(plist);
        return {};
    }

    std::vector<DeviceConfig> out;
    CFArrayRef arr = (CFArrayRef)plist;
    for (CFIndex i = 0; i < CFArrayGetCount(arr); ++i) {
        CFDictionaryRef d = (CFDictionaryRef)CFArrayGetValueAtIndex(arr, i);
        if (CFGetTypeID(d) != CFDictionaryGetTypeID()) continue;
        std::string id   = CFStr((CFStringRef)CFDictionaryGetValue(d, CFSTR("id")));
        std::string name = CFStr((CFStringRef)CFDictionaryGetValue(d, CFSTR("name")));
        if (!id.empty() && !name.empty()) out.push_back({id, name});
    }
    CFRelease(plist);
    return out;
}

// Keep rings + handlers alive for the lifetime of the driver.
static std::vector<std::unique_ptr<DeviceRing>>      gRings;
static std::vector<std::shared_ptr<SplitIOHandler>>  gHandlers;

static std::shared_ptr<aspl::Driver> CreateDriver() {
    auto context = std::make_shared<aspl::Context>();
    auto plugin  = std::make_shared<aspl::Plugin>(context);

    for (const auto& cfg : ReadConfig()) {
        auto ring    = std::make_unique<DeviceRing>(2);
        auto handler = std::make_shared<SplitIOHandler>(*ring);

        aspl::DeviceParameters params;
        params.Name         = cfg.name;
        params.Manufacturer = "Splitwave";
        params.DeviceUID    = "com.horuse.splitwave.audio." + cfg.id;
        params.ModelUID     = "com.horuse.splitwave.audio.model";
        params.SampleRate   = 48000;
        params.ChannelCount = 2;
        params.EnableMixing = true;

        auto device = std::make_shared<aspl::Device>(context, params);
        device->SetIOHandler(handler);
        device->SetControlHandler(handler);
        device->AddStreamWithControlsAsync(aspl::Direction::Output);
        device->AddStreamWithControlsAsync(aspl::Direction::Input);

        plugin->AddDevice(device);
        gHandlers.push_back(handler);
        gRings.push_back(std::move(ring));
    }

    return std::make_shared<aspl::Driver>(context, plugin);
}

extern "C" void* EntryPoint(CFAllocatorRef, CFUUIDRef typeUUID) {
    if (!CFEqual(typeUUID, kAudioServerPlugInTypeUUID)) return nullptr;
    static std::shared_ptr<aspl::Driver> driver = CreateDriver();
    return driver->GetReference();
}
