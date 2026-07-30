#include <aspl/Driver.hpp>
#include <aspl/Device.hpp>
#include <aspl/Plugin.hpp>
#include <aspl/IORequestHandler.hpp>
#include <aspl/ControlRequestHandler.hpp>
#include <CoreAudio/AudioServerPlugIn.h>
#include <CoreFoundation/CoreFoundation.h>
#include <dispatch/dispatch.h>
#include <fcntl.h>
#include <atomic>
#include <cmath>
#include <cstring>
#include <map>
#include <memory>
#include <mutex>
#include <string>
#include <vector>

// Writable by group staff so the app can swap it without root; coreaudiod reads it.
static const char* kConfigDir  = "/Library/Application Support/Splitwave";
static const char* kConfigPath = "/Library/Application Support/Splitwave/devices.plist";

struct DeviceRing {
    static constexpr uint32_t kFrames = 16384;
    uint32_t channels;
    uint32_t nSamples;
    std::vector<float> buf;
    std::atomic<int64_t> lastOutFrame{0};

    explicit DeviceRing(uint32_t ch)
        : channels(ch), nSamples(kFrames * ch), buf(kFrames * ch, 0.0f) {}
};

class SplitIOHandler : public aspl::IORequestHandler,
                       public aspl::ControlRequestHandler {
    // Owned, not borrowed: a removed device may still have IO in flight.
    std::shared_ptr<DeviceRing> ringOwner_;
    DeviceRing& ring_;
public:
    explicit SplitIOHandler(std::shared_ptr<DeviceRing> r)
        : ringOwner_(std::move(r)), ring_(*ringOwner_) {}

    void OnWriteMixedOutput(
        const std::shared_ptr<aspl::Stream>&,
        Float64, Float64 timestamp,
        const void* buff, UInt32 bytes) override
    {
        const float* src = static_cast<const float*>(buff);
        const uint32_t n = bytes / sizeof(float);
        const int64_t frame = llround(timestamp);
        const uint64_t base = (uint64_t)frame * ring_.channels;
        for (uint32_t i = 0; i < n; ++i) {
            ring_.buf[(base + i) % ring_.nSamples] = src[i];
        }
        ring_.lastOutFrame.store(frame + n / ring_.channels, std::memory_order_release);
    }

    void OnReadClientInput(
        const std::shared_ptr<aspl::Client>&,
        const std::shared_ptr<aspl::Stream>&,
        Float64, Float64 timestamp,
        void* buff, UInt32 bytes) override
    {
        float* dst = static_cast<float*>(buff);
        const uint32_t n = bytes / sizeof(float);
        const int64_t frame = llround(timestamp);
        const int64_t frames = n / ring_.channels;
        if (ring_.lastOutFrame.load(std::memory_order_acquire) - frames < frame) {
            memset(dst, 0, bytes);
            return;
        }
        const uint64_t base = (uint64_t)frame * ring_.channels;
        for (uint32_t i = 0; i < n; ++i) {
            dst[i] = ring_.buf[(base + i) % ring_.nSamples];
        }
    }
};

struct DeviceConfig { std::string id; std::string name; uint32_t channels; };

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
    CFURLRef url = CFURLCreateFromFileSystemRepresentation(
        nullptr, (const UInt8*)kConfigPath, strlen(kConfigPath), false);
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
        uint32_t channels = 2;
        CFNumberRef ch = (CFNumberRef)CFDictionaryGetValue(d, CFSTR("channels"));
        if (ch && CFGetTypeID(ch) == CFNumberGetTypeID()) {
            int v = 0;
            CFNumberGetValue(ch, kCFNumberIntType, &v);
            if (v >= 1 && v <= 256) channels = (uint32_t)v;
        }
        if (!id.empty() && !name.empty()) out.push_back({id, name, channels});
    }
    CFRelease(plist);
    return out;
}

struct DeviceEntry {
    std::shared_ptr<aspl::Device>   device;
    std::shared_ptr<SplitIOHandler> handler;
    uint32_t                        channels;
};

static std::shared_ptr<aspl::Context>     gContext;
static std::shared_ptr<aspl::Plugin>      gPlugin;
static std::map<std::string, DeviceEntry> gDevices;
static std::mutex                         gDevicesMutex;

// libASPL streams default to 16-bit int; our IO is float.
static AudioStreamBasicDescription FloatFormat(UInt32 channels) {
    AudioStreamBasicDescription f = {};
    f.mSampleRate       = 48000;
    f.mFormatID         = kAudioFormatLinearPCM;
    f.mFormatFlags      = kAudioFormatFlagIsFloat | kAudioFormatFlagsNativeEndian |
                          kAudioFormatFlagIsPacked;
    f.mBitsPerChannel   = 32;
    f.mChannelsPerFrame = channels;
    f.mBytesPerFrame    = channels * sizeof(float);
    f.mFramesPerPacket  = 1;
    f.mBytesPerPacket   = channels * sizeof(float);
    return f;
}

static DeviceEntry BuildDevice(const DeviceConfig& cfg) {
    auto ring    = std::make_shared<DeviceRing>(cfg.channels);
    auto handler = std::make_shared<SplitIOHandler>(ring);

    aspl::DeviceParameters params;
    params.Name         = cfg.name;
    params.Manufacturer = "Splitwave";
    params.DeviceUID    = "com.horuse.splitwave.audio." + cfg.id;
    params.ModelUID     = "com.horuse.splitwave.audio.model";
    params.SampleRate   = 48000;
    params.ChannelCount = cfg.channels;
    params.EnableMixing = true;

    auto device = std::make_shared<aspl::Device>(gContext, params);
    device->SetIOHandler(handler);
    device->SetControlHandler(handler);

    aspl::StreamParameters outStream;
    outStream.Direction = aspl::Direction::Output;
    outStream.Format = FloatFormat(params.ChannelCount);
    device->AddStreamWithControlsAsync(outStream);

    aspl::StreamParameters inStream;
    inStream.Direction = aspl::Direction::Input;
    inStream.Format = FloatFormat(params.ChannelCount);
    device->AddStreamWithControlsAsync(inStream);

    return {device, handler, cfg.channels};
}

// Reconciles the live device set against the config file. A channel count change
// cannot be applied in place, so it becomes a remove followed by an add.
static void SyncDevices() {
    const auto configs = ReadConfig();
    std::lock_guard<std::mutex> lock(gDevicesMutex);

    for (auto it = gDevices.begin(); it != gDevices.end();) {
        const auto* cfg = [&]() -> const DeviceConfig* {
            for (const auto& c : configs) if (c.id == it->first) return &c;
            return nullptr;
        }();
        if (!cfg || cfg->channels != it->second.channels
                 || cfg->name != it->second.device->GetName()) {
            gPlugin->RemoveDevice(it->second.device);
            it = gDevices.erase(it);
        } else {
            ++it;
        }
    }

    for (const auto& cfg : configs) {
        if (gDevices.count(cfg.id)) continue;
        auto entry = BuildDevice(cfg);
        gPlugin->AddDevice(entry.device);
        gDevices.emplace(cfg.id, std::move(entry));
    }
}

static void StartConfigWatcher();

// The config is replaced by atomic rename, so the file's inode dies with every
// write: the watch has to sit on the enclosing directory.
static void StartConfigWatcher() {
    int fd = open(kConfigDir, O_EVTONLY);
    if (fd < 0) return;

    dispatch_queue_t queue = dispatch_get_global_queue(QOS_CLASS_UTILITY, 0);
    dispatch_source_t src = dispatch_source_create(
        DISPATCH_SOURCE_TYPE_VNODE, fd, DISPATCH_VNODE_WRITE | DISPATCH_VNODE_RENAME
            | DISPATCH_VNODE_DELETE, queue);
    if (!src) { close(fd); return; }

    dispatch_source_set_event_handler(src, ^{
        const unsigned long flags = dispatch_source_get_data(src);
        SyncDevices();
        // The watched directory itself went away; re-arm on whatever replaced it.
        if (flags & (DISPATCH_VNODE_DELETE | DISPATCH_VNODE_RENAME)) {
            dispatch_source_cancel(src);
            dispatch_after(
                dispatch_time(DISPATCH_TIME_NOW, 500 * NSEC_PER_MSEC), queue, ^{
                    StartConfigWatcher();
                });
        }
    });
    dispatch_source_set_cancel_handler(src, ^{
        close(fd);
        dispatch_release(src);
    });
    dispatch_resume(src);
}

static std::shared_ptr<aspl::Driver> CreateDriver() {
    gContext = std::make_shared<aspl::Context>();
    gPlugin  = std::make_shared<aspl::Plugin>(gContext);

    auto driver = std::make_shared<aspl::Driver>(gContext, gPlugin);
    SyncDevices();
    StartConfigWatcher();
    return driver;
}

extern "C" void* EntryPoint(CFAllocatorRef, CFUUIDRef typeUUID) {
    if (!CFEqual(typeUUID, kAudioServerPlugInTypeUUID)) return nullptr;
    static std::shared_ptr<aspl::Driver> driver = CreateDriver();
    return driver->GetReference();
}
